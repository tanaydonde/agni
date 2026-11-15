//! Scenario assumptions derived from a CoStar record.
//!
//! Deterministic rules, not a model. Every value carries the CoStar field it
//! came from and why, and stays an unverified assumption a crew can override.
//! Facts CoStar cannot establish become verification items instead of inputs.

use serde::{Deserialize, Serialize};

use crate::{PropertyKind, PropertyRecord};

const SQFT_PER_M2: f64 = 10.763_910_4;

/// A derived scenario input with its provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Derived<T> {
    pub value: T,
    pub source_field: String,
    pub rationale: String,
}

fn derived<T>(value: T, source_field: &str, rationale: impl Into<String>) -> Derived<T> {
    Derived {
        value,
        source_field: source_field.to_owned(),
        rationale: rationale.into(),
    }
}

/// When occupants notice the fire (mirrors simulation-core's `Detected` alarm).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AlarmTimings {
    pub notice_hrr_kw: f64,
    pub same_room_seconds: f64,
    pub seconds_per_metre: f64,
}

/// Floor finish for the surface fire model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloorMaterial {
    Carpet,
    Hardwood,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationItem {
    pub item: String,
    pub reason: String,
    pub source_field: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireProfile {
    /// Who is in the scanned space, in words, for the occupant placer.
    pub occupant_context: Derived<String>,
    pub occupant_count: Derived<usize>,
    pub alarm: Derived<AlarmTimings>,
    /// Share of occupants who cannot leave unaided.
    pub injured_fraction: Derived<f64>,
    pub floor_material: Derived<FloorMaterial>,
    pub verification: Vec<VerificationItem>,
}

/// Facts about the scan that come from the scene, not CoStar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileInputs {
    /// Total room area of the scanned floor(s).
    pub scanned_area_m2: Option<f64>,
    /// Upper bound the simulation accepts.
    pub max_occupants: usize,
}

/// What the scanned space is used for, as far as CoStar says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpaceUse {
    Home,
    Childcare,
    Fitness,
    Office,
    Retail,
    Other,
}

impl SpaceUse {
    /// IBC Table 1004.5 occupant load factor, square feet per person.
    fn sqft_per_person(self) -> f64 {
        match self {
            Self::Childcare => 35.0,
            Self::Fitness => 50.0,
            Self::Office => 150.0,
            Self::Retail => 60.0,
            Self::Home | Self::Other => 200.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Childcare => "childcare / day care space",
            Self::Fitness => "exercise room",
            Self::Office => "business space",
            Self::Retail => "mercantile space",
            Self::Other => "residential-type space",
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Childcare => "childcare / day care (IBC 35 sq ft per person)",
            Self::Fitness => "exercise room (IBC 50 sq ft per person)",
            Self::Office => "business (IBC 150 sq ft per person)",
            Self::Retail => "mercantile (IBC 60 sq ft per person)",
            Self::Other => "residential-type space (IBC 200 sq ft per person)",
        }
    }
}

fn space_use(record: &PropertyRecord) -> (SpaceUse, &'static str) {
    if record.kind == PropertyKind::Residential {
        return (SpaceUse::Home, "residential-listings");
    }
    let caption = record
        .scan
        .description
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| caption.contains(w));
    if has(&[
        "child",
        "school",
        "daycare",
        "day care",
        "preschool",
        "nursery",
    ]) {
        return (SpaceUse::Childcare, "attachments[].caption");
    }
    if has(&["fitness", "gym"]) {
        return (SpaceUse::Fitness, "attachments[].caption");
    }
    let by_type = match record.property_type.as_deref() {
        Some("Office") => SpaceUse::Office,
        Some("GeneralRetail" | "Retail") => SpaceUse::Retail,
        _ => SpaceUse::Other,
    };
    (by_type, "propertyType")
}

/// Derives scenario assumptions and the crew verification list.
#[must_use]
pub fn derive(record: &PropertyRecord, inputs: ProfileInputs) -> FireProfile {
    let (space, use_field) = space_use(record);
    let cap = inputs.max_occupants.max(1);
    let scope = record
        .scan
        .description
        .as_deref()
        .unwrap_or("the scanned space");

    let (occupant_context, occupant_count) = if space == SpaceUse::Home {
        let beds = record.bedrooms;
        let count = beds.map_or(4, |b| (b as usize * 2).max(1)).min(cap);
        (
            derived(
                format!(
                    "A household asleep at night in a {}-bedroom {} built {}. People are in or near the bedrooms, a few may be in living areas.",
                    beds.map_or("?".into(), |b| b.to_string()),
                    // MLS types read "Residential Single-Family, Single Family Residence".
                    record
                        .property_type
                        .as_deref()
                        .and_then(|t| t.rsplit(',').next())
                        .map_or("home", str::trim)
                        .to_lowercase(),
                    record
                        .year_built
                        .map_or("at an unknown date".into(), |y| y.to_string()),
                ),
                "property.bedsMax",
                "Night-time scenario: the hardest case for a home, since occupants must wake before they move.",
            ),
            derived(
                count,
                "property.bedsMax",
                match beds {
                    Some(b) => format!("{b} bedrooms x 2 sleepers per bedroom."),
                    None => "Bedroom count unknown; assumed a household of 4.".into(),
                },
            ),
        )
    } else {
        let area_sqft = inputs.scanned_area_m2.map(|m2| m2 * SQFT_PER_M2);
        let count = area_sqft
            .map_or(10, |a| (a / space.sqft_per_person()).ceil() as usize)
            .clamp(1, cap);
        let who = match space {
            SpaceUse::Childcare => {
                "Young children with a few adult staff, during the daytime. Children cluster in the main activity rooms with staff nearby."
            }
            SpaceUse::Fitness => "Adults exercising, daytime.",
            SpaceUse::Office => {
                "Office workers at desks and in meeting rooms, during business hours."
            }
            SpaceUse::Retail => "Shoppers and staff, during opening hours.",
            _ => "Residents and visitors using a shared space, during the daytime.",
        };
        (
            derived(
                format!(
                    "{scope} at {}. {who}",
                    record.name.as_deref().unwrap_or("the property")
                ),
                use_field,
                format!("Space use read from CoStar as {}.", space.describe()),
            ),
            derived(
                count,
                use_field,
                match area_sqft {
                    Some(a) => format!(
                        "Code design load for a {}: {a:.0} sq ft scanned / {} sq ft per person. An upper bound, not a headcount.",
                        space.label(),
                        space.sqft_per_person()
                    ),
                    None => "Scanned area unknown; assumed 10 people.".into(),
                },
            ),
        )
    };

    let alarm = if space == SpaceUse::Home {
        derived(
            AlarmTimings {
                notice_hrr_kw: 100.0,
                same_room_seconds: 30.0,
                seconds_per_metre: 1.0,
            },
            "residential-listings (no smoke alarm field)",
            "Sleepers notice a larger fire and take longer to react and to wake others. CoStar records no smoke alarms.",
        )
    } else if space == SpaceUse::Childcare {
        derived(
            AlarmTimings {
                notice_hrr_kw: 50.0,
                same_room_seconds: 5.0,
                seconds_per_metre: 0.3,
            },
            use_field,
            "Awake staff notice quickly but need time to gather children before moving.",
        )
    } else {
        derived(
            AlarmTimings {
                notice_hrr_kw: 50.0,
                same_room_seconds: 2.0,
                seconds_per_metre: 0.2,
            },
            use_field,
            "Awake occupants during the daytime: simulation default.",
        )
    };

    let injured_fraction = match space {
        SpaceUse::Childcare => derived(
            0.3,
            use_field,
            "Young children often need an adult to get them out.",
        ),
        SpaceUse::Home => derived(
            if record.features.accessibility_features == Some(true) {
                0.25
            } else {
                0.15
            },
            "resoValues.hasAccessibilityFeatures",
            "A sleeping household may include young children or older adults who need help; the listing does not say.",
        ),
        _ => derived(0.1, use_field, "Simulation default."),
    };

    let floor_material = match record.features.hardwood_floors {
        Some(true) => derived(
            FloorMaterial::Hardwood,
            "resoValues.hasHardwoodFloors",
            "The listing reports hardwood floors.",
        ),
        _ => derived(
            FloorMaterial::Carpet,
            "resoValues.hasHardwoodFloors",
            "Floor finish not reported; the simulation default is carpet.",
        ),
    };

    FireProfile {
        occupant_context,
        occupant_count,
        alarm,
        injured_fraction,
        floor_material,
        verification: verification(record),
    }
}

fn verification(r: &PropertyRecord) -> Vec<VerificationItem> {
    let mut items = Vec::new();
    let mut add = |item: String, reason: &str, field: &str| {
        items.push(VerificationItem {
            item,
            reason: reason.to_owned(),
            source_field: field.to_owned(),
        });
    };
    if r.scan.partial {
        let rest = match (r.units, r.buildings) {
            (Some(u), Some(b)) => {
                format!(" The rest of the property ({u} units in {b} buildings) is not scanned.")
            }
            _ => String::new(),
        };
        add(
            format!(
                "The scan covers only {}.{rest}",
                r.scan.description.as_deref().unwrap_or("one space")
            ),
            "Anything outside the scan is unknown.",
            "attachments[].caption",
        );
    }
    let wood = r.kind == PropertyKind::Residential
        || r.construction_material
            .as_deref()
            .is_some_and(|m| m.contains("Wood"));
    match r.year_built {
        Some(y) if y < 1940 && wood => add(
            format!("Built in {y}: check for balloon framing."),
            "Pre-1940 wood frames often have open stud cavities from the foundation to the attic, so fire can travel up inside the walls unseen.",
            "yearBuilt",
        ),
        Some(y) if wood => add(
            format!("Wood-frame construction (built {y}): check for concealed combustible voids."),
            "Fire can spread through attics and floor voids before it is visible.",
            "buildingConstructionMaterial",
        ),
        _ => {}
    }
    match r.has_sprinklers {
        None => add(
            "Sprinkler presence unknown.".into(),
            "CoStar has no sprinkler record for this building.",
            "hasSprinklers",
        ),
        Some(false) => add(
            "No sprinklers recorded: confirm.".into(),
            "The simulation assumes no suppression.",
            "hasSprinklers",
        ),
        Some(true) => {}
    }
    if r.kind == PropertyKind::Residential {
        add(
            "Smoke alarm presence and operation.".into(),
            "Not in listing data; the scenario assumes slow notice by sleeping occupants.",
            "resoValues",
        );
    }
    if r.stories.is_some_and(|s| s > 1)
        && (r.is_walkup == Some(true) || r.has_elevators == Some(false))
    {
        add(
            format!(
                "Stair-only egress from {} stories.",
                r.stories.unwrap_or_default()
            ),
            "No elevators are recorded; upper-floor occupants who cannot walk need carrying.",
            "propertyApartment.isWalkup / elevators",
        );
    }
    if r.features.fireplace == Some(true) {
        add(
            "Fireplace and chimney condition.".into(),
            "A possible ignition source and a vertical path for fire.",
            "resoValues.hasFireplace",
        );
    }
    if r.features.basement == Some(true) {
        add(
            "Basement access and contents.".into(),
            "Basement fires are hard to reach and can undermine the floor above.",
            "resoValues.hasBasement",
        );
    }
    for c in &r.conflicts {
        add(
            format!("CoStar sources disagree on {}.", c.label.to_lowercase()),
            &c.values
                .iter()
                .map(|v| format!("{} = {}", v.source_field, v.value))
                .collect::<Vec<_>>()
                .join(" vs "),
            "conflicts",
        );
    }
    if !r.tenants.is_empty() {
        add(
            "Who actually occupies the building now.".into(),
            "The tenant list is CoStar research, not a headcount.",
            "location-occupancy",
        );
    }
    items
}
