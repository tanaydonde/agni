//! Adapter between canonical `world-core` scenes and NIST CFAST inputs.

mod export;
mod parser;
mod prepare;
mod runner;
mod scenario;

pub use export::{CfastExportError, export_all_rooms, export_single_room};
pub use parser::{CfastParseError, parse_cfast_results};
pub use prepare::{
    CfastPreparationError, CfastPreparationReport, CfastPreparationWarning, HeightInferenceSource,
    OpeningExclusionReason, PreparedCfastScene, RoomExclusionReason, prepare_cfast_scene,
};
pub use runner::{CFAST_SUPPORTED_VERSION, CfastRunError, CfastRunner};
pub use scenario::{CfastFire, CfastFuel, CfastOpeningState, CfastScenario, HeatReleasePoint};
