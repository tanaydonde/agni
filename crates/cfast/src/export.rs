use std::{
    collections::HashSet,
    fmt::{self, Write},
};

use tflop_world_core::{
    BoundaryEdge, Floor, Opening, OpeningConnection, OpeningTopologyError, Room, Scene,
    SceneValidationErrors, Vec3, resolve_opening_connections,
};
use thiserror::Error;

use crate::{CfastFire, CfastFuel, CfastRoomFire, CfastScenario, HeatReleasePoint};

const CFAST_INPUT_VERSION: u32 = 7700;
const POSITION_EPSILON_M: f64 = 1.0e-6;
const SCAN_DIMENSION_TOLERANCE_M: f64 = 0.10;

/// Exports the first vertical slice: one room, one exterior opening, and one
/// prescribed fire. The selected room is approximated by a CFAST rectangle.
pub fn export_single_room(
    scene: &Scene,
    scenario: &CfastScenario,
) -> Result<String, CfastExportError> {
    export_rooms(scene, scenario, RoomScope::Selected)
}

/// Exports every room as a CFAST compartment. The scenario's selected room
/// contains the fire and the currently supported exterior opening.
pub fn export_all_rooms(
    scene: &Scene,
    scenario: &CfastScenario,
) -> Result<String, CfastExportError> {
    export_rooms(scene, scenario, RoomScope::All)
}

#[derive(Clone, Copy)]
enum RoomScope {
    Selected,
    All,
}

fn export_rooms(
    scene: &Scene,
    scenario: &CfastScenario,
    scope: RoomScope,
) -> Result<String, CfastExportError> {
    scene.validate()?;
    validate_scenario(scenario)?;

    let (floor, room) = scene
        .floors
        .iter()
        .find_map(|floor| {
            floor
                .rooms
                .iter()
                .find(|room| room.id == scenario.room_id)
                .map(|room| (floor, room))
        })
        .ok_or_else(|| CfastExportError::RoomNotFound {
            room_id: scenario.room_id.clone(),
        })?;
    let selected = map_compartment(floor, room)?;
    let compartments = match scope {
        RoomScope::Selected => vec![selected],
        RoomScope::All => {
            let mut room_ids = HashSet::new();
            let mut compartments = Vec::new();
            for floor in &scene.floors {
                for room in &floor.rooms {
                    if !room_ids.insert(room.id.as_str()) {
                        return Err(CfastExportError::DuplicateRoomId {
                            room_id: room.id.clone(),
                        });
                    }
                    compartments.push(map_compartment(floor, room)?);
                }
            }
            compartments
        }
    };
    let selected = compartments
        .iter()
        .find(|compartment| compartment.room.id == scenario.room_id)
        .expect("the selected room was mapped above");
    let fires = map_fires(&scenario.fires, &compartments)?;

    let vents = match scope {
        RoomScope::Selected => {
            let (opening_edge, opening) = find_opening(room, &scenario.exterior_opening_id)
                .ok_or_else(|| CfastExportError::OpeningNotFound {
                    room_id: room.id.clone(),
                    opening_id: scenario.exterior_opening_id.clone(),
                })?;
            let open_fraction = scenario
                .opening_states
                .iter()
                .find(|state| state.opening_id == opening.id)
                .map_or(1.0, |state| state.open_fraction);
            vec![map_opening_vent(
                opening_edge,
                opening,
                selected,
                None,
                open_fraction,
            )?]
        }
        RoomScope::All => map_connection_vents(
            &resolve_opening_connections(scene)?,
            &compartments,
            scenario,
        )?,
    };

    render_input(scenario, &compartments, &fires, &vents)
}

#[derive(Debug, Error)]
pub enum CfastExportError {
    #[error(transparent)]
    InvalidScene(#[from] SceneValidationErrors),
    #[error(transparent)]
    InvalidOpeningTopology(#[from] OpeningTopologyError),
    #[error("room {room_id} was not found")]
    RoomNotFound { room_id: String },
    #[error("room ID {room_id} is not unique across the scene")]
    DuplicateRoomId { room_id: String },
    #[error("room {room_id} has no dimensions")]
    MissingRoomDimensions { room_id: String },
    #[error("room {room_id} has no height")]
    MissingRoomHeight { room_id: String },
    #[error("room {room_id} has invalid boundary geometry")]
    InvalidRoomGeometry { room_id: String },
    #[error("opening {opening_id} was not found in room {room_id}")]
    OpeningNotFound { room_id: String, opening_id: String },
    #[error("opening {opening_id} has no state in the CFAST scenario")]
    MissingOpeningState { opening_id: String },
    #[error("opening state {opening_id} does not match a resolved physical opening")]
    UnknownOpeningState { opening_id: String },
    #[error("opening {opening_id} has more than one state in the CFAST scenario")]
    DuplicateOpeningState { opening_id: String },
    #[error("fire {fire_id} is outside room {room_id}")]
    FireOutsideRoom { fire_id: String, room_id: String },
    #[error("fire {fire_id} references room {room_id}, which is not included in this export")]
    FireRoomNotIncluded { fire_id: String, room_id: String },
    #[error("invalid CFAST scenario field {field}: {message}")]
    InvalidScenario {
        field: &'static str,
        message: &'static str,
    },
    #[error("opening {opening_id} does not fit the rectangular CFAST compartment")]
    OpeningDoesNotFit { opening_id: String },
    #[error("failed to render CFAST input")]
    Render(#[source] fmt::Error),
}

#[derive(Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn width(self) -> f64 {
        self.max_x - self.min_x
    }

    fn depth(self) -> f64 {
        self.max_y - self.min_y
    }
}

struct CompartmentMapping<'a> {
    room: &'a Room,
    bounds: Bounds,
    width_m: f64,
    depth_m: f64,
    height_m: f64,
    floor_elevation_m: f64,
}

struct FireMapping<'a> {
    room_fire: &'a CfastRoomFire,
    x_m: f64,
    y_m: f64,
}

#[derive(Clone, Copy)]
enum VentFace {
    Front,
    Rear,
    Left,
    Right,
}

impl VentFace {
    const fn as_cfast(self) -> &'static str {
        match self {
            Self::Front => "FRONT",
            Self::Rear => "REAR",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }
}

struct VentMapping {
    id: String,
    room_a_id: String,
    room_b_id: Option<String>,
    width_m: f64,
    face: VentFace,
    bottom_m: f64,
    height_m: f64,
    offset_m: f64,
    open_fraction: f64,
}

fn render_input(
    scenario: &CfastScenario,
    compartments: &[CompartmentMapping<'_>],
    fires: &[FireMapping<'_>],
    vents: &[VentMapping],
) -> Result<String, CfastExportError> {
    let mut output = String::new();

    writeln!(
        output,
        "&HEAD VERSION = {CFAST_INPUT_VERSION}, TITLE = {} /\n",
        quote(&scenario.title)
    )
    .map_err(CfastExportError::Render)?;
    writeln!(
        output,
        "&TIME SIMULATION = {} PRINT = 10 SMOKEVIEW = 0 SPREADSHEET = {} /",
        number(scenario.duration_seconds),
        number(scenario.output_interval_seconds)
    )
    .map_err(CfastExportError::Render)?;
    writeln!(
        output,
        "&INIT PRESSURE = 101325 RELATIVE_HUMIDITY = {} INTERIOR_TEMPERATURE = {} EXTERIOR_TEMPERATURE = {} /\n",
        number(scenario.relative_humidity_percent),
        number(scenario.interior_temperature_c),
        number(scenario.exterior_temperature_c)
    )
    .map_err(CfastExportError::Render)?;

    let origin_x = compartments
        .iter()
        .map(|compartment| compartment.bounds.min_x)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let origin_y = compartments
        .iter()
        .map(|compartment| compartment.bounds.min_y)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let origin_z = compartments
        .iter()
        .map(|compartment| compartment.floor_elevation_m)
        .reduce(f64::min)
        .unwrap_or(0.0);

    for compartment in compartments {
        render_compartment(&mut output, compartment, origin_x, origin_y, origin_z)?;
    }

    for vent in vents {
        render_vent(&mut output, vent, scenario.duration_seconds)?;
    }

    for mapped in fires {
        let room_id = quote(&mapped.room_fire.room_id);
        let fire = &mapped.room_fire.fire;
        let fire_id = quote(&fire.id);
        let fire_properties_id = quote(&format!("{} Properties", fire.id));
        writeln!(
            output,
            "&FIRE ID = {fire_id} COMP_ID = {room_id} FIRE_ID = {fire_properties_id}"
        )
        .and_then(|()| {
            writeln!(
                output,
                "      LOCATION = {}, {} /",
                number(mapped.x_m),
                number(mapped.y_m)
            )
        })
        .map_err(CfastExportError::Render)?;
        render_fuel(&mut output, fire_properties_id.as_str(), fire.fuel)?;
        render_fire_curve(&mut output, fire_properties_id.as_str(), fire)?;
    }
    writeln!(output, "\n&TAIL /").map_err(CfastExportError::Render)?;

    Ok(output)
}

fn render_vent(
    output: &mut String,
    vent: &VentMapping,
    duration_seconds: f64,
) -> Result<(), CfastExportError> {
    let room_b_id = vent.room_b_id.as_deref().unwrap_or("OUTSIDE");
    writeln!(
        output,
        "&VENT TYPE = 'WALL' ID = {} COMP_IDS = {}, {}",
        quote(&vent.id),
        quote(&vent.room_a_id),
        quote(room_b_id)
    )
    .and_then(|()| {
        writeln!(
            output,
            "      BOTTOM = {} HEIGHT = {} WIDTH = {}",
            number(vent.bottom_m),
            number(vent.height_m),
            number(vent.width_m)
        )
    })
    .and_then(|()| {
        writeln!(
            output,
            "      CRITERION = 'TIME' T = 0, {} F = {}, {} FACE = '{}' OFFSET = {} /\n",
            number(duration_seconds),
            number(vent.open_fraction),
            number(vent.open_fraction),
            vent.face.as_cfast(),
            number(vent.offset_m)
        )
    })
    .map_err(CfastExportError::Render)
}

fn map_compartment<'a>(
    floor: &Floor,
    room: &'a Room,
) -> Result<CompartmentMapping<'a>, CfastExportError> {
    let bounds = room_bounds(room)?;
    let dimensions =
        room.dimensions
            .as_ref()
            .ok_or_else(|| CfastExportError::MissingRoomDimensions {
                room_id: room.id.clone(),
            })?;
    let width_m = dimensions.width_m.unwrap_or(bounds.width());
    let depth_m = dimensions.depth_m.unwrap_or(bounds.depth());
    let height_m = dimensions
        .height_m
        .ok_or_else(|| CfastExportError::MissingRoomHeight {
            room_id: room.id.clone(),
        })?;
    validate_positive("room width", width_m)?;
    validate_positive("room depth", depth_m)?;
    validate_positive("room height", height_m)?;

    Ok(CompartmentMapping {
        room,
        bounds,
        width_m,
        depth_m,
        height_m,
        floor_elevation_m: dimensions
            .lower_elevation_m
            .or(floor.elevation_m)
            .unwrap_or(bounds.min_z),
    })
}

fn map_fires<'a>(
    fires: &'a [CfastRoomFire],
    compartments: &[CompartmentMapping<'_>],
) -> Result<Vec<FireMapping<'a>>, CfastExportError> {
    fires
        .iter()
        .map(|room_fire| {
            let compartment = compartments
                .iter()
                .find(|compartment| compartment.room.id == room_fire.room_id)
                .ok_or_else(|| CfastExportError::FireRoomNotIncluded {
                    fire_id: room_fire.fire.id.clone(),
                    room_id: room_fire.room_id.clone(),
                })?;
            if !point_in_room(room_fire.fire.position_m, compartment.room) {
                return Err(CfastExportError::FireOutsideRoom {
                    fire_id: room_fire.fire.id.clone(),
                    room_id: room_fire.room_id.clone(),
                });
            }
            Ok(FireMapping {
                room_fire,
                x_m: scale_coordinate(
                    room_fire.fire.position_m.x,
                    compartment.bounds.min_x,
                    compartment.bounds.max_x,
                    compartment.width_m,
                ),
                y_m: scale_coordinate(
                    room_fire.fire.position_m.y,
                    compartment.bounds.min_y,
                    compartment.bounds.max_y,
                    compartment.depth_m,
                ),
            })
        })
        .collect()
}

fn render_compartment(
    output: &mut String,
    compartment: &CompartmentMapping<'_>,
    origin_x: f64,
    origin_y: f64,
    origin_z: f64,
) -> Result<(), CfastExportError> {
    writeln!(output, "&COMP ID = {}", quote(&compartment.room.id))
        .and_then(|()| {
            writeln!(
                output,
                "      DEPTH = {} HEIGHT = {} WIDTH = {}",
                number(compartment.depth_m),
                number(compartment.height_m),
                number(compartment.width_m)
            )
        })
        .and_then(|()| {
            writeln!(
                output,
                "      CEILING_MATL_ID = 'OFF' WALL_MATL_ID = 'OFF' FLOOR_MATL_ID = 'OFF'"
            )
        })
        .and_then(|()| {
            writeln!(
                output,
                "      ORIGIN = {}, {}, {} GRID = 20, 20, 12 /\n",
                number(compartment.bounds.min_x - origin_x),
                number(compartment.bounds.min_y - origin_y),
                number(compartment.floor_elevation_m - origin_z)
            )
        })
        .map_err(CfastExportError::Render)
}

fn render_fuel(
    output: &mut String,
    properties_id: &str,
    fuel: CfastFuel,
) -> Result<(), CfastExportError> {
    writeln!(output, "&CHEM ID = {properties_id}")
        .and_then(|()| {
            writeln!(
                output,
                "      CARBON = {} HYDROGEN = {} OXYGEN = {} NITROGEN = {} CHLORINE = {}",
                number(fuel.carbon),
                number(fuel.hydrogen),
                number(fuel.oxygen),
                number(fuel.nitrogen),
                number(fuel.chlorine)
            )
        })
        .and_then(|()| {
            writeln!(
                output,
                "      HEAT_OF_COMBUSTION = {} RADIATIVE_FRACTION = {} /",
                number(fuel.heat_of_combustion_kj_per_kg),
                number(fuel.radiative_fraction)
            )
        })
        .map_err(CfastExportError::Render)
}

fn render_fire_curve(
    output: &mut String,
    properties_id: &str,
    fire: &CfastFire,
) -> Result<(), CfastExportError> {
    writeln!(output, "&TABL ID = {properties_id}")
        .and_then(|()| {
            writeln!(
                output,
                "      LABELS = 'TIME', 'HRR', 'HEIGHT', 'AREA', 'CO_YIELD', 'SOOT_YIELD', 'HCN_YIELD', 'HCL_YIELD', 'TRACE_YIELD' /"
            )
        })
        .map_err(CfastExportError::Render)?;
    for point in &fire.heat_release_curve {
        writeln!(
            output,
            "&TABL ID = {properties_id}, DATA = {}, {}, 0, {}, {}, {}, 0, 0, 0 /",
            number(point.time_seconds),
            number(point.hrr_kw),
            number(fire.area_m2),
            number(fire.fuel.co_yield),
            number(fire.fuel.soot_yield)
        )
        .map_err(CfastExportError::Render)?;
    }
    Ok(())
}

fn validate_scenario(scenario: &CfastScenario) -> Result<(), CfastExportError> {
    validate_nonempty("id", &scenario.id)?;
    validate_nonempty("title", &scenario.title)?;
    validate_nonempty("room_id", &scenario.room_id)?;
    validate_nonempty("exterior_opening_id", &scenario.exterior_opening_id)?;
    validate_positive("duration_seconds", scenario.duration_seconds)?;
    validate_positive("output_interval_seconds", scenario.output_interval_seconds)?;
    if scenario.output_interval_seconds > scenario.duration_seconds {
        return Err(invalid(
            "output_interval_seconds",
            "must not exceed duration",
        ));
    }
    validate_range(
        "relative_humidity_percent",
        scenario.relative_humidity_percent,
        0.0,
        100.0,
    )?;
    validate_finite("interior_temperature_c", scenario.interior_temperature_c)?;
    validate_finite("exterior_temperature_c", scenario.exterior_temperature_c)?;
    let mut opening_ids = HashSet::new();
    for state in &scenario.opening_states {
        validate_nonempty("opening_states.opening_id", &state.opening_id)?;
        validate_range(
            "opening_states.open_fraction",
            state.open_fraction,
            0.0,
            1.0,
        )?;
        if !opening_ids.insert(state.opening_id.as_str()) {
            return Err(invalid(
                "opening_states.opening_id",
                "opening IDs must be unique",
            ));
        }
    }
    if scenario.fires.is_empty() {
        return Err(invalid("fires", "requires at least one fire"));
    }
    let mut fire_ids = HashSet::new();
    for room_fire in &scenario.fires {
        validate_nonempty("fires.room_id", &room_fire.room_id)?;
        validate_nonempty("fires.fire.id", &room_fire.fire.id)?;
        if !fire_ids.insert(room_fire.fire.id.as_str()) {
            return Err(invalid("fires.fire.id", "fire IDs must be unique"));
        }
        validate_positive("fires.fire.area_m2", room_fire.fire.area_m2)?;
        validate_finite("fires.fire.position_m.x", room_fire.fire.position_m.x)?;
        validate_finite("fires.fire.position_m.y", room_fire.fire.position_m.y)?;
        validate_finite("fires.fire.position_m.z", room_fire.fire.position_m.z)?;
        validate_fuel(room_fire.fire.fuel)?;
        validate_curve(
            &room_fire.fire.heat_release_curve,
            scenario.duration_seconds,
        )?;
    }
    Ok(())
}

fn validate_fuel(fuel: CfastFuel) -> Result<(), CfastExportError> {
    for (field, value) in [
        ("fire.fuel.carbon", fuel.carbon),
        ("fire.fuel.hydrogen", fuel.hydrogen),
        ("fire.fuel.oxygen", fuel.oxygen),
        ("fire.fuel.nitrogen", fuel.nitrogen),
        ("fire.fuel.chlorine", fuel.chlorine),
        ("fire.fuel.co_yield", fuel.co_yield),
        ("fire.fuel.soot_yield", fuel.soot_yield),
    ] {
        validate_nonnegative(field, value)?;
    }
    if fuel.carbon + fuel.hydrogen + fuel.oxygen + fuel.nitrogen + fuel.chlorine <= 0.0 {
        return Err(invalid(
            "fire.fuel",
            "must contain at least one chemical element",
        ));
    }
    validate_positive(
        "fire.fuel.heat_of_combustion_kj_per_kg",
        fuel.heat_of_combustion_kj_per_kg,
    )?;
    validate_range(
        "fire.fuel.radiative_fraction",
        fuel.radiative_fraction,
        0.0,
        1.0,
    )
}

fn validate_curve(
    curve: &[HeatReleasePoint],
    duration_seconds: f64,
) -> Result<(), CfastExportError> {
    if curve.len() < 2 {
        return Err(invalid(
            "fire.heat_release_curve",
            "requires at least two points",
        ));
    }
    if curve[0].time_seconds != 0.0 {
        return Err(invalid(
            "fire.heat_release_curve",
            "must start at zero seconds",
        ));
    }
    let mut previous_time = -1.0;
    for point in curve {
        validate_nonnegative("fire.heat_release_curve.time_seconds", point.time_seconds)?;
        validate_nonnegative("fire.heat_release_curve.hrr_kw", point.hrr_kw)?;
        if point.time_seconds <= previous_time {
            return Err(invalid(
                "fire.heat_release_curve",
                "times must be strictly increasing",
            ));
        }
        if point.time_seconds > duration_seconds {
            return Err(invalid(
                "fire.heat_release_curve.time_seconds",
                "must not exceed duration",
            ));
        }
        previous_time = point.time_seconds;
    }
    Ok(())
}

fn room_bounds(room: &Room) -> Result<Bounds, CfastExportError> {
    let mut points = room.boundary.iter().flat_map(|edge| [edge.start, edge.end]);
    let first = points
        .next()
        .ok_or_else(|| CfastExportError::InvalidRoomGeometry {
            room_id: room.id.clone(),
        })?;
    let bounds = points.fold(
        Bounds {
            min_x: first.x,
            min_y: first.y,
            min_z: first.z,
            max_x: first.x,
            max_y: first.y,
        },
        |mut bounds, point| {
            bounds.min_x = bounds.min_x.min(point.x);
            bounds.min_y = bounds.min_y.min(point.y);
            bounds.min_z = bounds.min_z.min(point.z);
            bounds.max_x = bounds.max_x.max(point.x);
            bounds.max_y = bounds.max_y.max(point.y);
            bounds
        },
    );
    if bounds.width() <= POSITION_EPSILON_M || bounds.depth() <= POSITION_EPSILON_M {
        Err(CfastExportError::InvalidRoomGeometry {
            room_id: room.id.clone(),
        })
    } else {
        Ok(bounds)
    }
}

fn find_opening<'a>(room: &'a Room, opening_id: &str) -> Option<(&'a BoundaryEdge, &'a Opening)> {
    room.boundary.iter().find_map(|edge| {
        edge.openings
            .iter()
            .find(|opening| opening.id == opening_id)
            .map(|opening| (edge, opening))
    })
}

fn map_opening_vent(
    edge: &BoundaryEdge,
    opening: &Opening,
    compartment: &CompartmentMapping<'_>,
    room_b_id: Option<&str>,
    open_fraction: f64,
) -> Result<VentMapping, CfastExportError> {
    let center = Vec3 {
        x: edge.start.x + (edge.end.x - edge.start.x) * opening.relative_center,
        y: edge.start.y + (edge.end.y - edge.start.y) * opening.relative_center,
        z: edge.start.z,
    };
    map_vent_geometry(
        &opening.id,
        &compartment.room.id,
        room_b_id,
        center,
        opening.width_m,
        opening.height_m,
        opening.lower_elevation_m,
        compartment,
        open_fraction,
    )
}

fn map_connection_vents(
    connections: &[OpeningConnection],
    compartments: &[CompartmentMapping<'_>],
    scenario: &CfastScenario,
) -> Result<Vec<VentMapping>, CfastExportError> {
    let mut used_states = HashSet::new();
    let mut vents = Vec::with_capacity(connections.len());
    for connection in connections {
        let matching_states = scenario
            .opening_states
            .iter()
            .enumerate()
            .filter(|(_, state)| connection.source_opening_ids.contains(&state.opening_id))
            .collect::<Vec<_>>();
        let [(state_index, state)] = matching_states.as_slice() else {
            return if matching_states.is_empty() {
                Err(CfastExportError::MissingOpeningState {
                    opening_id: connection.id.clone(),
                })
            } else {
                Err(CfastExportError::DuplicateOpeningState {
                    opening_id: connection.id.clone(),
                })
            };
        };
        used_states.insert(*state_index);
        let compartment = compartments
            .iter()
            .find(|compartment| compartment.room.id == connection.room_a_id)
            .expect("opening topology only references scene rooms");
        vents.push(map_vent_geometry(
            &connection.id,
            &connection.room_a_id,
            connection.room_b_id.as_deref(),
            connection.center_m,
            connection.width_m,
            connection.height_m,
            connection.lower_elevation_m,
            compartment,
            state.open_fraction,
        )?);
    }

    if let Some((_, state)) = scenario
        .opening_states
        .iter()
        .enumerate()
        .find(|(index, _)| !used_states.contains(index))
    {
        return Err(CfastExportError::UnknownOpeningState {
            opening_id: state.opening_id.clone(),
        });
    }
    Ok(vents)
}

#[allow(clippy::too_many_arguments)]
fn map_vent_geometry(
    opening_id: &str,
    room_a_id: &str,
    room_b_id: Option<&str>,
    center: Vec3,
    opening_width_m: f64,
    opening_height_m: Option<f64>,
    lower_elevation_m: Option<f64>,
    compartment: &CompartmentMapping<'_>,
    open_fraction: f64,
) -> Result<VentMapping, CfastExportError> {
    let face = nearest_face(center, compartment.bounds);
    let wall_length_m = match face {
        VentFace::Front | VentFace::Rear => compartment.width_m,
        VentFace::Left | VentFace::Right => compartment.depth_m,
    };
    if opening_width_m > wall_length_m + SCAN_DIMENSION_TOLERANCE_M {
        return Err(CfastExportError::OpeningDoesNotFit {
            opening_id: opening_id.to_owned(),
        });
    }
    let effective_width_m = opening_width_m.min(wall_length_m);
    let center_along_wall = match face {
        VentFace::Front | VentFace::Rear => scale_coordinate(
            center.x,
            compartment.bounds.min_x,
            compartment.bounds.max_x,
            compartment.width_m,
        ),
        VentFace::Left | VentFace::Right => scale_coordinate(
            center.y,
            compartment.bounds.min_y,
            compartment.bounds.max_y,
            compartment.depth_m,
        ),
    };
    let offset_m =
        (center_along_wall - effective_width_m / 2.0).clamp(0.0, wall_length_m - effective_width_m);
    let requested_height_m = opening_height_m.unwrap_or(compartment.height_m);
    let bottom_m = lower_elevation_m
        .map_or(0.0, |elevation| elevation - compartment.floor_elevation_m)
        .max(0.0);
    if requested_height_m <= 0.0
        || bottom_m + requested_height_m > compartment.height_m + SCAN_DIMENSION_TOLERANCE_M
    {
        return Err(CfastExportError::OpeningDoesNotFit {
            opening_id: opening_id.to_owned(),
        });
    }
    let height_m = requested_height_m.min(compartment.height_m - bottom_m);

    Ok(VentMapping {
        id: opening_id.to_owned(),
        room_a_id: room_a_id.to_owned(),
        room_b_id: room_b_id.map(str::to_owned),
        width_m: effective_width_m,
        face,
        bottom_m,
        height_m,
        offset_m,
        open_fraction,
    })
}

fn nearest_face(point: Vec3, bounds: Bounds) -> VentFace {
    let candidates = [
        ((point.y - bounds.min_y).abs(), VentFace::Front),
        ((point.y - bounds.max_y).abs(), VentFace::Rear),
        ((point.x - bounds.min_x).abs(), VentFace::Left),
        ((point.x - bounds.max_x).abs(), VentFace::Right),
    ];
    candidates
        .into_iter()
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .expect("four faces always exist")
        .1
}

fn scale_coordinate(value: f64, source_min: f64, source_max: f64, target_length: f64) -> f64 {
    ((value - source_min) / (source_max - source_min) * target_length).clamp(0.0, target_length)
}

fn point_in_room(point: Vec3, room: &Room) -> bool {
    let mut inside = false;
    for edge in &room.boundary {
        if point_on_edge(point, edge) {
            return true;
        }
        if (edge.start.y > point.y) != (edge.end.y > point.y) {
            let crossing_x = (edge.end.x - edge.start.x) * (point.y - edge.start.y)
                / (edge.end.y - edge.start.y)
                + edge.start.x;
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}

fn point_on_edge(point: Vec3, edge: &BoundaryEdge) -> bool {
    let cross = (edge.end.x - edge.start.x) * (point.y - edge.start.y)
        - (edge.end.y - edge.start.y) * (point.x - edge.start.x);
    cross.abs() <= POSITION_EPSILON_M
        && point.x >= edge.start.x.min(edge.end.x) - POSITION_EPSILON_M
        && point.x <= edge.start.x.max(edge.end.x) + POSITION_EPSILON_M
        && point.y >= edge.start.y.min(edge.end.y) - POSITION_EPSILON_M
        && point.y <= edge.start.y.max(edge.end.y) + POSITION_EPSILON_M
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), CfastExportError> {
    if value.trim().is_empty() {
        Err(invalid(field, "cannot be empty"))
    } else {
        Ok(())
    }
}

fn validate_finite(field: &'static str, value: f64) -> Result<(), CfastExportError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(invalid(field, "must be finite"))
    }
}

fn validate_positive(field: &'static str, value: f64) -> Result<(), CfastExportError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(invalid(field, "must be finite and greater than zero"))
    }
}

fn validate_nonnegative(field: &'static str, value: f64) -> Result<(), CfastExportError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(invalid(field, "must be finite and nonnegative"))
    }
}

fn validate_range(
    field: &'static str,
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), CfastExportError> {
    if value.is_finite() && (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(invalid(field, "value is outside the supported range"))
    }
}

fn invalid(field: &'static str, message: &'static str) -> CfastExportError {
    CfastExportError::InvalidScenario { field, message }
}
