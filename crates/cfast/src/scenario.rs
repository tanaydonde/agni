use serde::{Deserialize, Serialize};
use tflop_world_core::Vec3;

/// Most points CFAST 7.7.7 reads in one input curve (`mxpts` in
/// `cfast_parameters.f90`). A longer curve overruns its arrays and crashes
/// the solver.
pub const CFAST_MAX_CURVE_POINTS: usize = 200;

/// User-controlled inputs that are not present in the building scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfastScenario {
    pub id: String,
    pub title: String,
    pub room_id: String,
    pub exterior_opening_id: String,
    pub duration_seconds: f64,
    pub output_interval_seconds: f64,
    pub interior_temperature_c: f64,
    pub exterior_temperature_c: f64,
    pub relative_humidity_percent: f64,
    /// Initial state of every opening used by an all-room simulation.
    #[serde(default)]
    pub opening_states: Vec<CfastOpeningState>,
    pub fires: Vec<CfastRoomFire>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfastOpeningState {
    pub opening_id: String,
    /// Zero is fully closed and one is fully open.
    pub open_fraction: f64,
}

/// One user-prescribed fire and its heat-release curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfastFire {
    pub id: String,
    pub position_m: Vec3,
    pub area_m2: f64,
    pub fuel: CfastFuel,
    pub heat_release_curve: Vec<HeatReleasePoint>,
}

/// Associates one prescribed fire with its canonical room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfastRoomFire {
    pub room_id: String,
    pub fire: CfastFire,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeatReleasePoint {
    pub time_seconds: f64,
    /// CFAST accepts prescribed HRR values in kilowatts.
    pub hrr_kw: f64,
}

/// Simplified fuel chemistry used by CFAST to derive mass loss and products.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CfastFuel {
    pub carbon: f64,
    pub hydrogen: f64,
    pub oxygen: f64,
    pub nitrogen: f64,
    pub chlorine: f64,
    pub heat_of_combustion_kj_per_kg: f64,
    pub radiative_fraction: f64,
    pub co_yield: f64,
    pub soot_yield: f64,
}

impl Default for CfastFuel {
    fn default() -> Self {
        // Generic cellulosic fuel suitable for a feasibility scenario. Real
        // furniture must eventually select a measured fire curve and fuel.
        Self {
            carbon: 6.0,
            hydrogen: 10.0,
            oxygen: 5.0,
            nitrogen: 0.0,
            chlorine: 0.0,
            heat_of_combustion_kj_per_kg: 18_000.0,
            radiative_fraction: 0.35,
            co_yield: 0.006,
            soot_yield: 0.015,
        }
    }
}
