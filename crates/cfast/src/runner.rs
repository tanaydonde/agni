use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::tempdir;
use tflop_simulation_core::SimulationResults;
use tflop_world_core::Scene;
use thiserror::Error;

use crate::{
    CfastExportError, CfastParseError, CfastScenario, export_all_rooms, export_single_room,
    parse_cfast_results,
};

pub const CFAST_SUPPORTED_VERSION: &str = "7.7.7";
const PROJECT_NAME: &str = "scenario";

/// Synchronous adapter around the external CFAST command-line solver.
#[derive(Debug, Clone)]
pub struct CfastRunner {
    executable_path: PathBuf,
}

impl CfastRunner {
    pub fn new(executable_path: impl AsRef<Path>) -> Result<Self, CfastRunError> {
        let requested_path = executable_path.as_ref();
        let executable_path =
            requested_path
                .canonicalize()
                .map_err(|source| CfastRunError::Io {
                    operation: "resolve CFAST executable",
                    path: requested_path.to_path_buf(),
                    source,
                })?;
        if !executable_path.is_file() {
            return Err(CfastRunError::NotAFile {
                path: executable_path,
            });
        }
        Ok(Self { executable_path })
    }

    #[must_use]
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }

    /// Confirms that the configured binary is the version supported by the
    /// input exporter and CSV parser.
    pub fn verify_version(&self) -> Result<(), CfastRunError> {
        let output = Command::new(&self.executable_path)
            .output()
            .map_err(|source| CfastRunError::Io {
                operation: "launch CFAST version check",
                path: self.executable_path.clone(),
                source,
            })?;
        let combined = process_text(&output.stdout, &output.stderr);
        if !output.status.success() {
            return Err(CfastRunError::ProcessFailed {
                exit_code: output.status.code(),
                output: combined,
            });
        }
        let expected_marker = format!("CFAST-{CFAST_SUPPORTED_VERSION}");
        if !combined.contains(&expected_marker) {
            return Err(CfastRunError::UnsupportedVersion {
                expected: CFAST_SUPPORTED_VERSION,
                reported_output: combined,
            });
        }
        Ok(())
    }

    /// Generates input, runs CFAST, parses its spreadsheets, and returns a
    /// validated provider-independent timeline.
    pub fn run_single_room(
        &self,
        scene: &Scene,
        scenario: &CfastScenario,
    ) -> Result<SimulationResults, CfastRunError> {
        let input = export_single_room(scene, scenario)?;
        self.run_input(scene, scenario, input)
    }

    /// Runs one CFAST simulation containing every room in the scene. The rooms
    /// are not yet connected by interior vents.
    pub fn run_all_rooms(
        &self,
        scene: &Scene,
        scenario: &CfastScenario,
    ) -> Result<SimulationResults, CfastRunError> {
        let input = export_all_rooms(scene, scenario)?;
        self.run_input(scene, scenario, input)
    }

    fn run_input(
        &self,
        scene: &Scene,
        scenario: &CfastScenario,
        input: String,
    ) -> Result<SimulationResults, CfastRunError> {
        self.verify_version()?;
        let working_directory = tempdir().map_err(|source| CfastRunError::Io {
            operation: "create CFAST working directory",
            path: std::env::temp_dir(),
            source,
        })?;
        let input_path = working_directory.path().join(format!("{PROJECT_NAME}.in"));
        fs::write(&input_path, input).map_err(|source| CfastRunError::Io {
            operation: "write CFAST input",
            path: input_path.clone(),
            source,
        })?;

        let output = Command::new(&self.executable_path)
            .arg(format!("{PROJECT_NAME}.in"))
            .current_dir(working_directory.path())
            .output()
            .map_err(|source| CfastRunError::Io {
                operation: "launch CFAST simulation",
                path: self.executable_path.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(CfastRunError::ProcessFailed {
                exit_code: output.status.code(),
                output: process_text(&output.stdout, &output.stderr),
            });
        }

        let compartments_path = working_directory
            .path()
            .join(format!("{PROJECT_NAME}_compartments.csv"));
        let vents_path = working_directory
            .path()
            .join(format!("{PROJECT_NAME}_vents.csv"));
        let compartments_csv = read_output(&compartments_path)?;
        let vents_csv = read_output(&vents_path)?;
        parse_cfast_results(
            scene,
            scenario,
            CFAST_SUPPORTED_VERSION,
            &compartments_csv,
            &vents_csv,
        )
        .map_err(CfastRunError::Parse)
    }
}

#[derive(Debug, Error)]
pub enum CfastRunError {
    #[error("CFAST executable path is not a file: {path}")]
    NotAFile { path: PathBuf },
    #[error("could not {operation} at {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("expected CFAST {expected}, but the executable reported:\n{reported_output}")]
    UnsupportedVersion {
        expected: &'static str,
        reported_output: String,
    },
    #[error("CFAST process failed with exit code {exit_code:?}:\n{output}")]
    ProcessFailed {
        exit_code: Option<i32>,
        output: String,
    },
    #[error(transparent)]
    Export(#[from] CfastExportError),
    #[error(transparent)]
    Parse(#[from] CfastParseError),
}

fn read_output(path: &Path) -> Result<String, CfastRunError> {
    fs::read_to_string(path).map_err(|source| CfastRunError::Io {
        operation: "read CFAST output",
        path: path.to_path_buf(),
        source,
    })
}

fn process_text(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    match (stdout.trim(), stderr.trim()) {
        ("", "") => "(no process output)".to_owned(),
        (stdout, "") => stdout.to_owned(),
        ("", stderr) => stderr.to_owned(),
        (stdout, stderr) => format!("{stdout}\n{stderr}"),
    }
}
