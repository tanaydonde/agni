//! Adapter between canonical `world-core` scenes and NIST CFAST inputs.

mod export;
mod merge;
mod parser;
mod prepare;
mod runner;
mod scenario;
mod surface_adapter;

pub use export::{CfastExportError, export_all_rooms, export_single_room};
pub use merge::{CfastMergeError, merge_cfast_with_surface_fire};
pub use parser::{CfastParseError, parse_cfast_results};
pub use prepare::{
    CfastPreparationError, CfastPreparationReport, CfastPreparationWarning, HeightInferenceSource,
    OpeningExclusionReason, PreparedCfastScene, RoomExclusionReason, prepare_cfast_scene,
};
pub use runner::{CFAST_SUPPORTED_VERSION, CfastRunError, CfastRunner};
pub use scenario::{
    CfastFire, CfastFuel, CfastOpeningState, CfastRoomFire, CfastScenario, HeatReleasePoint,
};
pub use surface_adapter::{
    CfastSurfaceFireTimeline, SurfaceFireAdapterConfig, SurfaceFireAdapterError,
    adapt_surface_fire_for_cfast,
};
