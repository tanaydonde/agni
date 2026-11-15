//! Occupant placement. The default `spread` layout is Gemini-backed: Gemini
//! only proposes tool arguments, and simulation-core remains the authority
//! that accepts or rejects them. The `crowded` layout packs people into the
//! largest rooms deterministically, without Gemini, so a demo or test can
//! count on queues at the doors.

use std::time::Duration;
use std::{env, future::Future, pin::Pin};

use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tflop_simulation_core::{
    AgentFrame, DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S, GridCoordinate, OCCUPANT_RADIUS_M,
    OccupantSpawn, ScenarioValidationErrors, Simulation, SimulationConfig, SimulationError,
    reachable_exit_cells,
};
use tflop_world_core::Vec3;

use crate::{SharedProperty, SharedWorld, decode, lock};

const DEFAULT_GEMINI_MODEL: &str = "gemini-3.5-flash-lite";
const MAX_ATTEMPTS: usize = 3;
pub(crate) const MAX_OCCUPANTS: usize = 200;
const MAX_SPAWN_SITES_FOR_MODEL: usize = 2_000;
/// Hurried evacuation walking speeds, for crowded layouts and the prompt.
const MIN_HURRIED_SPEED_M_PER_S: f64 = 1.2;
const MAX_HURRIED_SPEED_M_PER_S: f64 = 1.8;
/// Crowded layouts start everyone moving within this long, so they meet at
/// the doors together.
const MAX_CROWDED_DELAY_SECONDS: f64 = 15.0;

#[derive(Debug, Deserialize)]
pub(crate) struct SpawnOccupantsRequest {
    count: usize,
    #[serde(default)]
    layout: OccupantLayout,
}

/// How occupants are placed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OccupantLayout {
    /// Gemini spreads people plausibly across rooms.
    #[default]
    Spread,
    /// Packed into the largest rooms first, without Gemini.
    Crowded,
}

#[derive(Debug, Serialize)]
struct SpawnOccupantsResponse {
    /// World revision used to validate the generated layout.
    revision: u64,
    attempts: usize,
    occupants: Vec<OccupantSpawn>,
    agents: Vec<AgentFrame>,
}

#[derive(Debug)]
struct GeneratedOccupants {
    attempts: usize,
    occupants: Vec<OccupantSpawn>,
    agents: Vec<AgentFrame>,
}

#[derive(Debug, Clone)]
struct ProposalRequest {
    count: usize,
    attempt: usize,
    spawn_sites: Vec<SpawnSite>,
    feedback: Option<String>,
    /// Who is in the building, from its CoStar record, when one is loaded.
    building_context: Option<String>,
}

/// A Rust-verified position Gemini may select. Sites are laid out farther
/// apart than two occupant radii, so distinct site IDs cannot collide.
#[derive(Debug, Clone, Serialize)]
struct SpawnSite {
    id: String,
    floor_id: String,
    position_m: Vec3,
    room_ids: Vec<String>,
}

trait OccupantProposalProvider {
    fn propose(
        &self,
        request: ProposalRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<OccupantSpawn>, AgentError>> + Send + '_>>;
}

pub(crate) async fn spawn_occupants(
    State(world): State<SharedWorld>,
    Extension(property): Extension<SharedProperty>,
    body: Result<Json<SpawnOccupantsRequest>, JsonRejection>,
) -> Response {
    let request = match decode(body) {
        Ok(request) => request,
        Err(rejection) => return rejection.into_response(),
    };
    if request.count == 0 || request.count > MAX_OCCUPANTS {
        return error_response(AgentError::InvalidCount {
            count: request.count,
        });
    }

    let snapshot = lock(&world).get_scene();
    // Only Gemini placement reads the building context; crowded skips it.
    let gemini_placement = request.layout != OccupantLayout::Crowded;
    let building_context = property
        .as_ref()
        .as_ref()
        .filter(|_| gemini_placement)
        .map(|brief| {
            let rooms = snapshot
                .scene
                .floors
                .iter()
                .flat_map(|floor| &floor.rooms)
                .filter_map(|room| {
                    let label = room
                        .label
                        .clone()
                        .filter(|label| !label.is_empty())
                        .or_else(|| room.classifications.first().map(|c| c.label.clone()))?;
                    Some(format!("{}={label}", room.id))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} Room labels by room id: {rooms}.",
                brief.profile.occupant_context.value
            )
        });
    let mut simulation = match Simulation::new(snapshot.scene, SimulationConfig::default()) {
        Ok(simulation) => simulation,
        Err(error) => return error_response(AgentError::Simulation(error.to_string())),
    };
    if request.layout == OccupantLayout::Crowded {
        return match spawn_crowded(&mut simulation, request.count) {
            Ok(generated) => Json(SpawnOccupantsResponse {
                revision: snapshot.revision,
                attempts: generated.attempts,
                occupants: generated.occupants,
                agents: generated.agents,
            })
            .into_response(),
            Err(error) => error_response(error),
        };
    }
    let provider = match GeminiProvider::from_env() {
        Ok(provider) => provider,
        Err(error) => return error_response(error),
    };

    match generate_and_spawn(&mut simulation, request.count, building_context, &provider).await {
        Ok(generated) => Json(SpawnOccupantsResponse {
            revision: snapshot.revision,
            attempts: generated.attempts,
            occupants: generated.occupants,
            agents: generated.agents,
        })
        .into_response(),
        Err(error) => error_response(error),
    }
}

async fn generate_and_spawn<P: OccupantProposalProvider>(
    simulation: &mut Simulation,
    count: usize,
    building_context: Option<String>,
    provider: &P,
) -> Result<GeneratedOccupants, AgentError> {
    let spawn_sites = build_spawn_sites(simulation, count)?;
    let mut feedback = None;
    let mut last_failure = "Gemini did not produce a placement".to_owned();

    for attempt in 1..=MAX_ATTEMPTS {
        let occupants = match provider
            .propose(ProposalRequest {
                count,
                attempt,
                spawn_sites: spawn_sites.clone(),
                feedback: feedback.clone(),
                building_context: building_context.clone(),
            })
            .await
        {
            Ok(occupants) => occupants,
            Err(error @ AgentError::GeminiRequest(_))
            | Err(error @ AgentError::GeminiApi { .. })
            | Err(error @ AgentError::NotConfigured(_)) => return Err(error),
            Err(error) => {
                last_failure = error.to_string();
                feedback = Some(last_failure.clone());
                continue;
            }
        };

        if occupants.len() != count {
            last_failure = format!(
                "spawn_occupants requires exactly {count} occupants, but the tool call supplied {}",
                occupants.len()
            );
            feedback = Some(last_failure.clone());
            continue;
        }

        match simulation.spawn_occupants(&occupants) {
            Ok(agents) => {
                return Ok(GeneratedOccupants {
                    attempts: attempt,
                    occupants,
                    agents,
                });
            }
            Err(SimulationError::InvalidScenario(errors)) => {
                last_failure = format_validation_feedback(&errors);
                feedback = Some(last_failure.clone());
            }
            Err(error) => return Err(AgentError::Simulation(error.to_string())),
        }
    }

    Err(AgentError::AttemptsExhausted {
        attempts: MAX_ATTEMPTS,
        last_failure,
    })
}

/// Packs `count` occupants into the rooms with the most spawn sites, each
/// room filled from its middle outward before the next, with hurried speeds
/// and short pre-movement delays.
fn spawn_crowded(
    simulation: &mut Simulation,
    count: usize,
) -> Result<GeneratedOccupants, AgentError> {
    let sites = build_spawn_sites(simulation, count)?;
    let mut rooms = Vec::<(String, Vec<SpawnSite>)>::new();
    for site in sites {
        let room_id = site.room_ids.first().cloned().unwrap_or_default();
        match rooms.iter_mut().find(|(id, _)| *id == room_id) {
            Some((_, room_sites)) => room_sites.push(site),
            None => rooms.push((room_id, vec![site])),
        }
    }
    rooms.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));

    let mut chosen = Vec::with_capacity(count);
    for (_, mut room_sites) in rooms {
        let center = centroid(&room_sites);
        room_sites.sort_by(|left, right| {
            planar_distance(left.position_m, center)
                .total_cmp(&planar_distance(right.position_m, center))
                .then_with(|| left.id.cmp(&right.id))
        });
        chosen.extend(room_sites.into_iter().take(count - chosen.len()));
        if chosen.len() == count {
            break;
        }
    }

    let occupants = chosen
        .into_iter()
        .enumerate()
        .map(|(index, site)| {
            let number = index as u64 + 1;
            OccupantSpawn {
                id: format!("occupant-{number}"),
                floor_id: site.floor_id,
                position_m: site.position_m,
                walking_speed_m_per_s: MIN_HURRIED_SPEED_M_PER_S
                    + (MAX_HURRIED_SPEED_M_PER_S - MIN_HURRIED_SPEED_M_PER_S) * unit(number, 1),
                pre_movement_delay_seconds: MAX_CROWDED_DELAY_SECONDS * unit(number, 2),
                injured: false,
            }
        })
        .collect::<Vec<_>>();
    let agents = simulation
        .spawn_occupants(&occupants)
        .map_err(|error| AgentError::Simulation(error.to_string()))?;
    Ok(GeneratedOccupants {
        attempts: 0,
        occupants,
        agents,
    })
}

fn centroid(sites: &[SpawnSite]) -> Vec3 {
    let count = sites.len().max(1) as f64;
    Vec3 {
        x: sites.iter().map(|site| site.position_m.x).sum::<f64>() / count,
        y: sites.iter().map(|site| site.position_m.y).sum::<f64>() / count,
        z: 0.0,
    }
}

fn planar_distance(left: Vec3, right: Vec3) -> f64 {
    (left.x - right.x).hypot(left.y - right.y)
}

/// A repeatable number in `[0, 1)` for an occupant and a purpose.
fn unit(number: u64, purpose: u64) -> f64 {
    let mut z = number
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(purpose);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1_u64 << 53) as f64
}

fn build_spawn_sites(
    simulation: &Simulation,
    requested_count: usize,
) -> Result<Vec<SpawnSite>, AgentError> {
    let reachable = reachable_exit_cells(simulation.scene(), simulation.config())
        .map_err(|error| AgentError::Simulation(error.to_string()))?;
    let mut sites = Vec::new();
    for (floor_index, grid) in simulation.grids().iter().enumerate() {
        // One spare cell beyond two body radii gives the validator numerical
        // headroom and makes every pair of distinct sites non-overlapping.
        let stride = ((2.0 * OCCUPANT_RADIUS_M / grid.cell_size_m).ceil() as usize + 1).max(1);
        for row in (0..grid.rows).step_by(stride) {
            for column in (0..grid.columns).step_by(stride) {
                let coordinate = GridCoordinate { column, row };
                let Some(cell) = grid.cell(coordinate) else {
                    continue;
                };
                if !grid.circle_is_walkable(cell.center_m, OCCUPANT_RADIUS_M) {
                    continue;
                }
                if !reachable.contains(&(grid.floor_id.clone(), coordinate)) {
                    continue;
                }
                sites.push(SpawnSite {
                    id: format!("site-{floor_index}-{row}-{column}"),
                    floor_id: grid.floor_id.clone(),
                    position_m: cell.center_m,
                    room_ids: cell.room_ids.clone(),
                });
            }
        }
    }

    if sites.len() < requested_count {
        return Err(AgentError::InsufficientSpawnSites {
            requested: requested_count,
            available: sites.len(),
        });
    }
    if sites.len() > MAX_SPAWN_SITES_FOR_MODEL {
        let step = sites.len() as f64 / MAX_SPAWN_SITES_FOR_MODEL as f64;
        sites = (0..MAX_SPAWN_SITES_FOR_MODEL)
            .map(|index| sites[(index as f64 * step).floor() as usize].clone())
            .collect();
    }
    Ok(sites)
}

fn format_validation_feedback(errors: &ScenarioValidationErrors) -> String {
    let details = errors
        .0
        .iter()
        .map(|error| format!("{}: {}", error.path, error.message))
        .collect::<Vec<_>>()
        .join("; ");
    format!("spawn_occupants rejected the placement: {details}")
}

struct GeminiProvider {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

/// Whether `spread` placement can run (Gemini is configured).
pub(crate) fn gemini_configured() -> bool {
    GeminiProvider::from_env().is_ok()
}

impl GeminiProvider {
    fn from_env() -> Result<Self, AgentError> {
        let api_key = env::var("GEMINI_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| AgentError::NotConfigured("GEMINI_API_KEY is not set".to_owned()))?;
        let model = env::var("GEMINI_MODEL").unwrap_or_else(|_| DEFAULT_GEMINI_MODEL.to_owned());
        if model.is_empty()
            || !model
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
        {
            return Err(AgentError::NotConfigured(
                "GEMINI_MODEL contains unsupported characters".to_owned(),
            ));
        }
        Ok(Self {
            api_key,
            model,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|error| AgentError::NotConfigured(error.to_string()))?,
        })
    }

    fn request_body(request: &ProposalRequest) -> Value {
        let retry = request.feedback.as_deref().map_or_else(
            || "This is the first attempt.".to_owned(),
            |feedback| {
                format!(
                    "The previous tool call was rejected. Correct every issue and call the tool again. Feedback: {feedback}"
                )
            },
        );
        let building = request.building_context.as_deref().map_or_else(String::new, |context| {
            format!(
                "\n\nBuilding context from the CoStar property record: {context} Place people where that context makes them plausible (use the room labels to pick fitting rooms), and choose speeds and pre-movement delays that fit who they are."
            )
        });
        let sites_json = serde_json::to_string(&request.spawn_sites)
            .expect("spawn sites contain only serializable values");
        let prompt = format!(
            "Place exactly {count} plausible occupants in this building for the beginning of a fire evacuation training scenario. Select {count} DISTINCT spawn_site_id values from the verified catalog below; never invent or alter a site ID. Spread people naturally across rooms rather than clustering them. Give every occupant a unique stable ID occupant-1 through occupant-{count}. Use hurried evacuation walking speeds between {min_speed} and {max_speed} m/s and varied nonnegative pre-movement delays. This is attempt {attempt} of {MAX_ATTEMPTS}. {retry}{building}\n\nVerified spawn-site catalog JSON:\n{sites}",
            count = request.count,
            attempt = request.attempt,
            min_speed = MIN_HURRIED_SPEED_M_PER_S,
            max_speed = MAX_HURRIED_SPEED_M_PER_S,
            sites = sites_json,
        );

        json!({
            "systemInstruction": {
                "parts": [{
                    "text": "You generate bounded inputs for a deterministic evacuation simulator. You must call spawn_occupants using only supplied verified spawn site IDs and never claim that rejected inputs succeeded."
                }]
            },
            "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
            "tools": [{
                "functionDeclarations": [{
                    "name": "spawn_occupants",
                    "description": "Atomically validate and spawn the complete requested occupant layout.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "occupants": {
                                "type": "array",
                                "description": format!("Exactly {} occupant spawn records", request.count),
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "spawn_site_id": { "type": "string" },
                                        "walking_speed_m_per_s": { "type": "number" },
                                        "pre_movement_delay_seconds": { "type": "number" }
                                    },
                                    "required": ["id", "spawn_site_id"]
                                }
                            }
                        },
                        "required": ["occupants"]
                    }
                }]
            }],
            "toolConfig": {
                "functionCallingConfig": {
                    "mode": "ANY",
                    "allowedFunctionNames": ["spawn_occupants"]
                }
            },
            "generationConfig": { "temperature": 0.7 }
        })
    }

    fn parse_response(
        body: &Value,
        spawn_sites: &[SpawnSite],
    ) -> Result<Vec<OccupantSpawn>, AgentError> {
        let parts = body
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentError::InvalidToolCall("response has no candidate parts".into()))?;
        let call = parts
            .iter()
            .filter_map(|part| part.get("functionCall"))
            .find(|call| call.get("name").and_then(Value::as_str) == Some("spawn_occupants"))
            .ok_or_else(|| {
                AgentError::InvalidToolCall("Gemini did not call spawn_occupants".into())
            })?;
        let args = call
            .get("args")
            .cloned()
            .ok_or_else(|| AgentError::InvalidToolCall("tool call has no arguments".into()))?;
        let arguments: SpawnOccupantsArguments = serde_json::from_value(args)
            .map_err(|error| AgentError::InvalidToolCall(error.to_string()))?;
        arguments
            .occupants
            .into_iter()
            .map(|occupant| {
                let site = spawn_sites
                    .iter()
                    .find(|site| site.id == occupant.spawn_site_id)
                    .ok_or_else(|| {
                        AgentError::InvalidToolCall(format!(
                            "unknown spawn_site_id {}",
                            occupant.spawn_site_id
                        ))
                    })?;
                Ok(OccupantSpawn {
                    id: occupant.id,
                    floor_id: site.floor_id.clone(),
                    position_m: site.position_m,
                    walking_speed_m_per_s: occupant.walking_speed_m_per_s,
                    pre_movement_delay_seconds: occupant.pre_movement_delay_seconds,
                    injured: false,
                })
            })
            .collect()
    }
}

impl OccupantProposalProvider for GeminiProvider {
    fn propose(
        &self,
        request: ProposalRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<OccupantSpawn>, AgentError>> + Send + '_>> {
        Box::pin(async move {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                self.model
            );
            let response = self
                .http
                .post(url)
                .header("x-goog-api-key", &self.api_key)
                .json(&Self::request_body(&request))
                .send()
                .await
                .map_err(|error| AgentError::GeminiRequest(error.to_string()))?;
            let status = response.status();
            let body: Value = response
                .json()
                .await
                .map_err(|error| AgentError::GeminiRequest(error.to_string()))?;
            if !status.is_success() {
                let message = body
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Gemini returned an unknown error")
                    .to_owned();
                return Err(AgentError::GeminiApi {
                    status: status.as_u16(),
                    message,
                });
            }
            Self::parse_response(&body, &request.spawn_sites)
        })
    }
}

#[derive(Debug, Deserialize)]
struct SpawnOccupantsArguments {
    occupants: Vec<ProposedOccupant>,
}

#[derive(Debug, Deserialize)]
struct ProposedOccupant {
    id: String,
    spawn_site_id: String,
    #[serde(default = "default_walking_speed")]
    walking_speed_m_per_s: f64,
    #[serde(default)]
    pre_movement_delay_seconds: f64,
}

const fn default_walking_speed() -> f64 {
    DEFAULT_OCCUPANT_WALKING_SPEED_M_PER_S
}

#[derive(Debug)]
enum AgentError {
    InvalidCount {
        count: usize,
    },
    NotConfigured(String),
    InsufficientSpawnSites {
        requested: usize,
        available: usize,
    },
    GeminiRequest(String),
    GeminiApi {
        status: u16,
        message: String,
    },
    InvalidToolCall(String),
    Simulation(String),
    AttemptsExhausted {
        attempts: usize,
        last_failure: String,
    },
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCount { count } => write!(
                formatter,
                "count must be between 1 and {MAX_OCCUPANTS}, got {count}"
            ),
            Self::NotConfigured(message)
            | Self::GeminiRequest(message)
            | Self::InvalidToolCall(message)
            | Self::Simulation(message) => formatter.write_str(message),
            Self::InsufficientSpawnSites {
                requested,
                available,
            } => write!(
                formatter,
                "requested {requested} occupants, but only {available} verified spawn sites are available"
            ),
            Self::GeminiApi { status, message } => {
                write!(formatter, "Gemini returned HTTP {status}: {message}")
            }
            Self::AttemptsExhausted {
                attempts,
                last_failure,
            } => write!(
                formatter,
                "Gemini failed to produce a valid layout after {attempts} attempts: {last_failure}"
            ),
        }
    }
}

fn error_response(error: AgentError) -> Response {
    let (status, code) = match &error {
        AgentError::InvalidCount { .. } => (StatusCode::BAD_REQUEST, "invalid_count"),
        AgentError::NotConfigured(_) => (StatusCode::SERVICE_UNAVAILABLE, "gemini_not_configured"),
        AgentError::GeminiApi { status: 429, .. } => {
            (StatusCode::TOO_MANY_REQUESTS, "gemini_rate_limited")
        }
        AgentError::GeminiRequest(_) | AgentError::GeminiApi { .. } => {
            (StatusCode::BAD_GATEWAY, "gemini_failed")
        }
        AgentError::AttemptsExhausted { .. }
        | AgentError::InvalidToolCall(_)
        | AgentError::InsufficientSpawnSites { .. } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "occupant_generation_failed",
        ),
        AgentError::Simulation(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    (
        status,
        Json(json!({ "code": code, "message": error.to_string() })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use tflop_world_core::Vec3;

    use super::*;
    use crate::{DEMO_SCENE_JSON, load_scene};

    struct ScriptedProvider {
        proposals: Mutex<VecDeque<Vec<OccupantSpawn>>>,
        requests: Mutex<Vec<ProposalRequest>>,
    }

    impl ScriptedProvider {
        fn new(proposals: Vec<Vec<OccupantSpawn>>) -> Self {
            Self {
                proposals: Mutex::new(proposals.into()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl OccupantProposalProvider for ScriptedProvider {
        fn propose(
            &self,
            request: ProposalRequest,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<OccupantSpawn>, AgentError>> + Send + '_>>
        {
            Box::pin(async move {
                self.requests.lock().expect("requests lock").push(request);
                self.proposals
                    .lock()
                    .expect("proposals lock")
                    .pop_front()
                    .ok_or_else(|| AgentError::InvalidToolCall("script exhausted".into()))
            })
        }
    }

    #[tokio::test]
    async fn retries_with_validation_feedback_then_spawns() {
        let provider = ScriptedProvider::new(vec![
            vec![
                occupant("occupant-1", 1.0, 1.0),
                occupant("occupant-2", 1.2, 1.0),
            ],
            vec![
                occupant("occupant-1", 1.0, 1.0),
                occupant("occupant-2", 2.0, 1.0),
            ],
        ]);
        let mut simulation = simulation();

        let generated = generate_and_spawn(&mut simulation, 2, None, &provider)
            .await
            .expect("second proposal should pass");

        assert_eq!(generated.attempts, 2);
        assert_eq!(simulation.agents().len(), 2);
        let requests = provider.requests.lock().expect("requests lock");
        assert!(requests[0].feedback.is_none());
        assert!(
            requests[1]
                .feedback
                .as_deref()
                .is_some_and(|feedback| feedback.contains("overlaps another agent"))
        );
    }

    #[tokio::test]
    async fn stops_after_three_invalid_attempts_without_spawning() {
        let invalid = || {
            vec![
                occupant("occupant-1", 1.0, 1.0),
                occupant("occupant-2", 1.2, 1.0),
            ]
        };
        let provider = ScriptedProvider::new(vec![invalid(), invalid(), invalid()]);
        let mut simulation = simulation();

        let error = generate_and_spawn(&mut simulation, 2, None, &provider)
            .await
            .expect_err("all proposals should fail");

        assert!(matches!(
            error,
            AgentError::AttemptsExhausted { attempts: 3, .. }
        ));
        assert!(simulation.agents().is_empty());
        assert_eq!(provider.requests.lock().expect("requests lock").len(), 3);
    }

    #[test]
    fn parses_spawn_occupants_function_call() {
        let body = json!({
            "candidates": [{ "content": { "parts": [{
                "functionCall": {
                    "name": "spawn_occupants",
                    "args": { "occupants": [{
                        "id": "occupant-1",
                        "spawn_site_id": "site-1"
                    }] }
                }
            }] } }]
        });
        let sites = [SpawnSite {
            id: "site-1".to_owned(),
            floor_id: "floor-1".to_owned(),
            position_m: Vec3 {
                x: 1.0,
                y: 1.0,
                z: 0.0,
            },
            room_ids: vec!["room-1".to_owned()],
        }];

        let occupants = GeminiProvider::parse_response(&body, &sites).expect("call should parse");
        assert_eq!(occupants.len(), 1);
        assert_eq!(occupants[0].id, "occupant-1");
        assert_eq!(occupants[0].position_m, sites[0].position_m);
    }

    #[test]
    fn generated_spawn_sites_are_valid_and_mutually_separated() {
        let simulation = simulation();
        let sites = build_spawn_sites(&simulation, 2).expect("demo has spawn sites");

        assert!(sites.len() >= 2);
        for (index, left) in sites.iter().enumerate() {
            let grid = simulation.grid(&left.floor_id).expect("site floor exists");
            assert!(grid.circle_is_walkable(left.position_m, OCCUPANT_RADIUS_M));
            for right in &sites[index + 1..] {
                if left.floor_id != right.floor_id {
                    continue;
                }
                let dx = left.position_m.x - right.position_m.x;
                let dy = left.position_m.y - right.position_m.y;
                assert!(dx * dx + dy * dy >= 4.0 * OCCUPANT_RADIUS_M * OCCUPANT_RADIUS_M);
            }
        }
    }

    #[test]
    fn the_prompt_carries_the_costar_building_context_only_when_loaded() {
        let request = |building_context: Option<String>| ProposalRequest {
            count: 2,
            attempt: 1,
            spawn_sites: Vec::new(),
            feedback: None,
            building_context,
        };
        let text = |body: Value| {
            body["contents"][0]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let with = text(GeminiProvider::request_body(&request(Some(
            "A household asleep at night. Room labels by room id: r1=Bedroom.".into(),
        ))));
        assert!(
            with.contains("CoStar property record: A household asleep"),
            "{with}"
        );
        assert!(with.contains("r1=Bedroom"));
        let without = text(GeminiProvider::request_body(&request(None)));
        assert!(!without.contains("CoStar"));
    }

    #[test]
    fn crowded_layouts_are_dense_repeatable_and_hurried() {
        let mut first = simulation();
        let generated = spawn_crowded(&mut first, 12).expect("crowded layout spawns");
        assert_eq!(generated.occupants.len(), 12);
        assert_eq!(first.agents().len(), 12);
        let again = spawn_crowded(&mut simulation(), 12).expect("crowded layout spawns");
        assert_eq!(generated.occupants, again.occupants);
        for occupant in &generated.occupants {
            assert!(
                (MIN_HURRIED_SPEED_M_PER_S..MAX_HURRIED_SPEED_M_PER_S)
                    .contains(&occupant.walking_speed_m_per_s)
            );
            assert!(
                (0.0..MAX_CROWDED_DELAY_SECONDS).contains(&occupant.pre_movement_delay_seconds)
            );
        }
        // Packed from the middle of a room outward: nobody is far from the
        // rest.
        let center = Vec3 {
            x: generated
                .occupants
                .iter()
                .map(|o| o.position_m.x)
                .sum::<f64>()
                / 12.0,
            y: generated
                .occupants
                .iter()
                .map(|o| o.position_m.y)
                .sum::<f64>()
                / 12.0,
            z: 0.0,
        };
        assert!(
            generated
                .occupants
                .iter()
                .all(|occupant| planar_distance(occupant.position_m, center) < 2.0)
        );
    }

    fn simulation() -> Simulation {
        Simulation::new(
            load_scene(DEMO_SCENE_JSON).expect("demo scene"),
            SimulationConfig::default(),
        )
        .expect("simulation")
    }

    fn occupant(id: &str, x: f64, y: f64) -> OccupantSpawn {
        OccupantSpawn {
            id: id.to_owned(),
            floor_id: "floor-1".to_owned(),
            position_m: Vec3 { x, y, z: 0.0 },
            walking_speed_m_per_s: 1.2,
            pre_movement_delay_seconds: 0.0,
            injured: false,
        }
    }
}
