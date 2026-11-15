//! CoStar property data for a scanned building.
//!
//! The CoStar hackathon kit links every Matterport model it covers to either a
//! commercial property or a residential MLS listing. This crate joins a
//! Matterport model id to that record (`join`), keeps only the fields a fire
//! preplan needs (`PropertyRecord`; contacts are never copied), and derives
//! scenario assumptions from them (`profile`). Each derived value names the
//! CoStar field it came from and stays an unverified assumption.

pub mod join;
pub mod profile;

use serde::{Deserialize, Serialize};

pub use join::{JoinError, find_by_model_id};
pub use profile::{
    AlarmTimings, Derived, FireProfile, FloorMaterial, ProfileInputs, VerificationItem, derive,
};

/// Bumped when `PropertyRecord`'s shape changes incompatibly.
pub const PROPERTY_SCHEMA_VERSION: u32 = 1;

/// The CoStar facts for the building around one Matterport scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyRecord {
    pub schema_version: u32,
    pub source: RecordSource,
    pub kind: PropertyKind,
    /// What the scan covers. A scan of one room never stands for the building.
    pub scan: ScanScope,
    pub name: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Decoded CoStar property type, or the MLS type for a home.
    pub property_type: Option<String>,
    pub year_built: Option<i32>,
    pub stories: Option<u32>,
    pub area_sqft: Option<f64>,
    pub bedrooms: Option<u32>,
    pub bathrooms: Option<f64>,
    pub construction_material: Option<String>,
    /// `None` means unknown, never "no".
    pub has_sprinklers: Option<bool>,
    pub sprinkler_type: Option<String>,
    pub has_elevators: Option<bool>,
    pub is_walkup: Option<bool>,
    pub units: Option<u32>,
    pub buildings: Option<u32>,
    pub features: Features,
    /// Current tenants (commercial only). Employee counts are context, not an
    /// occupant count for the scanned space.
    pub tenants: Vec<Tenant>,
    /// Fields where CoStar's sources disagree.
    pub conflicts: Vec<Conflict>,
    /// Display list: every fact above with the CoStar field it came from.
    pub facts: Vec<Fact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordSource {
    pub provider: String,
    /// Kit dataset, e.g. `residential-listings` or `commercial-property`.
    pub dataset: String,
    pub record_id: i64,
    /// Date the record's data reflects (listing date or scan capture date).
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyKind {
    Residential,
    Commercial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanScope {
    pub model_id: String,
    /// CoStar's caption for the scan, e.g. "After School Care".
    pub description: Option<String>,
    /// True when the scan is one space inside a larger property.
    pub partial: bool,
    pub captured: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Features {
    pub fireplace: Option<bool>,
    pub fireplaces: Option<u32>,
    pub hardwood_floors: Option<bool>,
    pub basement: Option<bool>,
    pub garage: Option<bool>,
    pub architectural_style: Option<String>,
    pub accessibility_features: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tenant {
    pub name: Option<String>,
    pub industry: Option<String>,
    pub employees: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub label: String,
    pub values: Vec<Fact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
    /// CoStar field path, e.g. `resoValues.structure.yearBuilt`.
    pub source_field: String,
}
