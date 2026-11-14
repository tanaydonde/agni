use csv::{ReaderBuilder, StringRecord, Trim};
use tflop_simulation_core::{
    FireConditions, GasLayerConditions, OpeningConditions, ResultsValidationErrors, RoomConditions,
    SIMULATION_RESULTS_SCHEMA_VERSION, SimulationEngine, SimulationFrame, SimulationResults,
};
use tflop_world_core::Scene;
use thiserror::Error;

use crate::CfastScenario;

const COMPARTMENTS_FILE: &str = "compartments";
const VENTS_FILE: &str = "vents";
const TIME_TOLERANCE_SECONDS: f64 = 1.0e-6;

/// Converts CFAST 7.7.7 spreadsheet output into the canonical fire-results IR.
pub fn parse_cfast_results(
    scene: &Scene,
    scenario: &CfastScenario,
    engine_version: &str,
    compartments_csv: &str,
    vents_csv: &str,
) -> Result<SimulationResults, CfastParseError> {
    let mut frames = parse_compartments(compartments_csv, scenario)?;
    let opening_frames = parse_vents(vents_csv)?;

    if frames.len() != opening_frames.len() {
        return Err(CfastParseError::TimelineLengthMismatch {
            compartment_frames: frames.len(),
            vent_frames: opening_frames.len(),
        });
    }
    for (frame, (vent_time, openings)) in frames.iter_mut().zip(opening_frames) {
        if (frame.time_seconds - vent_time).abs() > TIME_TOLERANCE_SECONDS {
            return Err(CfastParseError::TimelineTimeMismatch {
                compartment_time: frame.time_seconds,
                vent_time,
            });
        }
        frame.openings = openings;
    }

    let results = SimulationResults {
        schema_version: SIMULATION_RESULTS_SCHEMA_VERSION,
        scene_id: scene.id.clone(),
        scenario_id: scenario.id.clone(),
        engine: SimulationEngine {
            name: "cfast".to_owned(),
            version: engine_version.to_owned(),
        },
        frames,
    };
    results.validate_against_scene(scene)?;
    Ok(results)
}

#[derive(Debug, Error)]
pub enum CfastParseError {
    #[error("could not parse CFAST {file} CSV")]
    Csv {
        file: &'static str,
        #[source]
        source: csv::Error,
    },
    #[error("CFAST {file} CSV needs four header rows and at least one data row")]
    MissingRows { file: &'static str },
    #[error("CFAST {file} CSV is missing {measurement} for {entity_id}")]
    MissingColumn {
        file: &'static str,
        measurement: &'static str,
        entity_id: String,
    },
    #[error("CFAST {file} CSV row {row} has no column {column}")]
    MissingValue {
        file: &'static str,
        row: usize,
        column: usize,
    },
    #[error("CFAST {file} CSV row {row}, column {column} contains invalid number {value:?}")]
    InvalidNumber {
        file: &'static str,
        row: usize,
        column: usize,
        value: String,
        #[source]
        source: std::num::ParseFloatError,
    },
    #[error(
        "CFAST timelines have different lengths: {compartment_frames} compartment frames and {vent_frames} vent frames"
    )]
    TimelineLengthMismatch {
        compartment_frames: usize,
        vent_frames: usize,
    },
    #[error(
        "CFAST timeline mismatch: compartment time {compartment_time} and vent time {vent_time}"
    )]
    TimelineTimeMismatch {
        compartment_time: f64,
        vent_time: f64,
    },
    #[error(transparent)]
    InvalidResults(#[from] ResultsValidationErrors),
}

#[derive(Clone, Copy)]
struct RoomColumns {
    upper_temperature: usize,
    lower_temperature: usize,
    layer_height: usize,
    gauge_pressure: usize,
    absolute_pressure: usize,
    upper_oxygen: usize,
    upper_co: usize,
    upper_hcn: usize,
    upper_optical_density: usize,
    lower_oxygen: usize,
    lower_co: usize,
    lower_hcn: usize,
    lower_optical_density: usize,
}

#[derive(Clone, Copy)]
struct FireColumns {
    ignited: usize,
    pyrolysis_rate: usize,
    expected_hrr: usize,
    actual_hrr: usize,
    convective_hrr: usize,
    flame_height: usize,
}

fn parse_compartments(
    input: &str,
    scenario: &CfastScenario,
) -> Result<Vec<SimulationFrame>, CfastParseError> {
    let records = read_records(input, COMPARTMENTS_FILE)?;
    let (codes, ids, data) = split_records(&records, COMPARTMENTS_FILE)?;
    let time = find_time_column(codes, COMPARTMENTS_FILE)?;
    let mut rooms = Vec::new();
    for room_id in unique_entity_ids(codes, ids, "ULT_") {
        rooms.push((room_id, room_columns(codes, ids, room_id)?));
    }
    if rooms.is_empty() {
        return Err(CfastParseError::MissingColumn {
            file: COMPARTMENTS_FILE,
            measurement: "upper temperature",
            entity_id: "any compartment".to_owned(),
        });
    }
    let fires = scenario
        .fires
        .iter()
        .map(|room_fire| {
            let fire_id = room_fire.fire.id.as_str();
            Ok((
                room_fire,
                FireColumns {
                    ignited: find_column(
                        codes,
                        ids,
                        fire_id,
                        "IGN_",
                        "ignition",
                        COMPARTMENTS_FILE,
                    )?,
                    pyrolysis_rate: find_column(
                        codes,
                        ids,
                        fire_id,
                        "PYROL_",
                        "pyrolysis rate",
                        COMPARTMENTS_FILE,
                    )?,
                    expected_hrr: find_column(
                        codes,
                        ids,
                        fire_id,
                        "HRR_E",
                        "expected HRR",
                        COMPARTMENTS_FILE,
                    )?,
                    actual_hrr: find_column(
                        codes,
                        ids,
                        fire_id,
                        "HRR_",
                        "actual HRR",
                        COMPARTMENTS_FILE,
                    )?,
                    convective_hrr: find_column(
                        codes,
                        ids,
                        fire_id,
                        "HRRC_",
                        "convective HRR",
                        COMPARTMENTS_FILE,
                    )?,
                    flame_height: find_column(
                        codes,
                        ids,
                        fire_id,
                        "FLHGT_",
                        "flame height",
                        COMPARTMENTS_FILE,
                    )?,
                },
            ))
        })
        .collect::<Result<Vec<_>, CfastParseError>>()?;

    data.iter()
        .enumerate()
        .map(|(index, record)| {
            let row = index + 5;
            let rooms = rooms
                .iter()
                .map(|(room_id, columns)| room_conditions(record, row, room_id, *columns))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SimulationFrame {
                time_seconds: value(record, time, row, COMPARTMENTS_FILE)?,
                rooms,
                fires: fires
                    .iter()
                    .map(|(room_fire, columns)| {
                        Ok(FireConditions {
                            fire_id: room_fire.fire.id.clone(),
                            room_id: room_fire.room_id.clone(),
                            ignited: value(record, columns.ignited, row, COMPARTMENTS_FILE)? > 0.5,
                            expected_hrr_kw: watts_to_kilowatts(value(
                                record,
                                columns.expected_hrr,
                                row,
                                COMPARTMENTS_FILE,
                            )?),
                            actual_hrr_kw: watts_to_kilowatts(value(
                                record,
                                columns.actual_hrr,
                                row,
                                COMPARTMENTS_FILE,
                            )?),
                            convective_hrr_kw: watts_to_kilowatts(value(
                                record,
                                columns.convective_hrr,
                                row,
                                COMPARTMENTS_FILE,
                            )?),
                            flame_height_m: value(
                                record,
                                columns.flame_height,
                                row,
                                COMPARTMENTS_FILE,
                            )?,
                            pyrolysis_rate_kg_per_s: value(
                                record,
                                columns.pyrolysis_rate,
                                row,
                                COMPARTMENTS_FILE,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, CfastParseError>>()?,
                openings: Vec::new(),
                surface_fire: Vec::new(),
            })
        })
        .collect()
}

fn unique_entity_ids<'a>(
    codes: &'a StringRecord,
    ids: &'a StringRecord,
    code_prefix: &str,
) -> Vec<&'a str> {
    let mut seen = std::collections::HashSet::new();
    codes
        .iter()
        .zip(ids.iter())
        .filter_map(|(code, id)| {
            (code.starts_with(code_prefix) && !id.is_empty() && seen.insert(id)).then_some(id)
        })
        .collect()
}

fn room_columns(
    codes: &StringRecord,
    ids: &StringRecord,
    room_id: &str,
) -> Result<RoomColumns, CfastParseError> {
    let find = |prefix, measurement| {
        find_column(codes, ids, room_id, prefix, measurement, COMPARTMENTS_FILE)
    };
    Ok(RoomColumns {
        upper_temperature: find("ULT_", "upper temperature")?,
        lower_temperature: find("LLT_", "lower temperature")?,
        layer_height: find("HGT_", "layer height")?,
        gauge_pressure: find("PRS_", "gauge pressure")?,
        absolute_pressure: find("APRS_", "absolute pressure")?,
        upper_oxygen: find("ULO2_", "upper oxygen")?,
        upper_co: find("ULCO_", "upper carbon monoxide")?,
        upper_hcn: find("ULHCN_", "upper hydrogen cyanide")?,
        upper_optical_density: find("ULOD_", "upper optical density")?,
        lower_oxygen: find("LLO2_", "lower oxygen")?,
        lower_co: find("LLCO_", "lower carbon monoxide")?,
        lower_hcn: find("LLHCN_", "lower hydrogen cyanide")?,
        lower_optical_density: find("LLOD_", "lower optical density")?,
    })
}

fn room_conditions(
    record: &StringRecord,
    row: usize,
    room_id: &str,
    columns: RoomColumns,
) -> Result<RoomConditions, CfastParseError> {
    Ok(RoomConditions {
        room_id: room_id.to_owned(),
        smoke_layer_height_m: value(record, columns.layer_height, row, COMPARTMENTS_FILE)?,
        gauge_pressure_pa: value(record, columns.gauge_pressure, row, COMPARTMENTS_FILE)?,
        absolute_pressure_pa: value(record, columns.absolute_pressure, row, COMPARTMENTS_FILE)?,
        upper_layer: GasLayerConditions {
            temperature_c: value(record, columns.upper_temperature, row, COMPARTMENTS_FILE)?,
            oxygen_percent: value(record, columns.upper_oxygen, row, COMPARTMENTS_FILE)?,
            carbon_monoxide_percent: value(record, columns.upper_co, row, COMPARTMENTS_FILE)?,
            hydrogen_cyanide_percent: value(record, columns.upper_hcn, row, COMPARTMENTS_FILE)?,
            optical_density_per_m: value(
                record,
                columns.upper_optical_density,
                row,
                COMPARTMENTS_FILE,
            )?,
        },
        lower_layer: GasLayerConditions {
            temperature_c: value(record, columns.lower_temperature, row, COMPARTMENTS_FILE)?,
            oxygen_percent: value(record, columns.lower_oxygen, row, COMPARTMENTS_FILE)?,
            carbon_monoxide_percent: value(record, columns.lower_co, row, COMPARTMENTS_FILE)?,
            hydrogen_cyanide_percent: value(record, columns.lower_hcn, row, COMPARTMENTS_FILE)?,
            optical_density_per_m: value(
                record,
                columns.lower_optical_density,
                row,
                COMPARTMENTS_FILE,
            )?,
        },
    })
}

fn parse_vents(input: &str) -> Result<Vec<(f64, Vec<OpeningConditions>)>, CfastParseError> {
    let records = read_records(input, VENTS_FILE)?;
    let (codes, ids, data) = split_records(&records, VENTS_FILE)?;
    let time = find_time_column(codes, VENTS_FILE)?;
    let openings = unique_entity_ids(codes, ids, "W_")
        .into_iter()
        .map(|opening_id| {
            Ok((
                opening_id,
                find_column(codes, ids, opening_id, "W_", "net flow", VENTS_FILE)?,
                find_column(
                    codes,
                    ids,
                    opening_id,
                    "WF_",
                    "opening fraction",
                    VENTS_FILE,
                )?,
            ))
        })
        .collect::<Result<Vec<_>, CfastParseError>>()?;

    data.iter()
        .enumerate()
        .map(|(index, record)| {
            let row = index + 5;
            let conditions = openings
                .iter()
                .map(|(opening_id, net_flow, opening_fraction)| {
                    Ok(OpeningConditions {
                        opening_id: (*opening_id).to_owned(),
                        net_flow_kg_per_s: value(record, *net_flow, row, VENTS_FILE)?,
                        opening_fraction: value(record, *opening_fraction, row, VENTS_FILE)?,
                    })
                })
                .collect::<Result<Vec<_>, CfastParseError>>()?;
            Ok((value(record, time, row, VENTS_FILE)?, conditions))
        })
        .collect()
}

fn read_records(input: &str, file: &'static str) -> Result<Vec<StringRecord>, CfastParseError> {
    ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(Trim::All)
        .from_reader(input.as_bytes())
        .records()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CfastParseError::Csv { file, source })
}

fn split_records<'a>(
    records: &'a [StringRecord],
    file: &'static str,
) -> Result<(&'a StringRecord, &'a StringRecord, &'a [StringRecord]), CfastParseError> {
    if records.len() < 5 {
        return Err(CfastParseError::MissingRows { file });
    }
    Ok((&records[0], &records[2], &records[4..]))
}

fn find_time_column(codes: &StringRecord, file: &'static str) -> Result<usize, CfastParseError> {
    codes
        .iter()
        .position(|code| code == "Time")
        .ok_or_else(|| CfastParseError::MissingColumn {
            file,
            measurement: "time",
            entity_id: "timeline".to_owned(),
        })
}

fn find_column(
    codes: &StringRecord,
    ids: &StringRecord,
    entity_id: &str,
    code_prefix: &str,
    measurement: &'static str,
    file: &'static str,
) -> Result<usize, CfastParseError> {
    codes
        .iter()
        .zip(ids.iter())
        .position(|(code, id)| code.starts_with(code_prefix) && id == entity_id)
        .ok_or_else(|| CfastParseError::MissingColumn {
            file,
            measurement,
            entity_id: entity_id.to_owned(),
        })
}

fn value(
    record: &StringRecord,
    column: usize,
    row: usize,
    file: &'static str,
) -> Result<f64, CfastParseError> {
    let raw = record
        .get(column)
        .ok_or(CfastParseError::MissingValue { file, row, column })?;
    raw.trim()
        .parse::<f64>()
        .map_err(|source| CfastParseError::InvalidNumber {
            file,
            row,
            column,
            value: raw.to_owned(),
            source,
        })
}

const fn watts_to_kilowatts(value: f64) -> f64 {
    value / 1_000.0
}
