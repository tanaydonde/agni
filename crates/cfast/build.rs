//! Compiles the vendored CFAST 7.7.7 solver with gfortran so a fresh clone can
//! run fire simulations without installing CFAST separately.
//!
//! On success, `TFLOP_CFAST_BUNDLED_BIN` is set at compile time to the built
//! executable's path. If no Fortran compiler is available, the build still
//! succeeds with a warning and the variable is left unset; the server then
//! reports how to fix it when a CFAST simulation is requested.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

const SOURCE_DIR: &str = "vendor/cfast-7.7.7/Source";

/// What upstream's makefile would embed via `git describe --long --dirty`.
/// The runner checks the solver's banner for `CFAST-7.7.7`.
const CFAST_GIT_DESCRIBE: &str = "CFAST-7.7.7-0-gb43e74e-vendored";
const CFAST_GIT_DATE: &str = "Mon May 25 09:11:41 2026 -0400";

/// Compile order from upstream's `objfgnu` list: each module must be built
/// before the files that `use` it.
const SOURCES: &[&str] = &[
    "prec.f90",
    "cfast_parameters.f90",
    "cfast_structures.f90",
    "cfast_data.f90",
    "convection.f90",
    "keyboard_gnu.f90",
    "srev.f90",
    "cfastexit.f90",
    "utilities.f90",
    "ssHeaders.f90",
    "outputdebug.f90",
    "compartments.f90",
    "conduction.f90",
    "flowfan.f90",
    "flowhorizontal.f90",
    "flowvertical.f90",
    "fire.f90",
    "numerics.f90",
    "radiation.f90",
    "target.f90",
    "outputsmv.f90",
    "outputspreadsheet.f90",
    "output.f90",
    "solve.f90",
    "initialization.f90",
    "input_namelist.f90",
    "input.f90",
    "cfast.f90",
];

fn main() {
    println!("cargo:rerun-if-changed={SOURCE_DIR}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FC");

    let compiler = env::var("FC").unwrap_or_else(|_| "gfortran".to_owned());
    if Command::new(&compiler).arg("--version").output().is_err() {
        println!(
            "cargo:warning=CFAST was not built because `{compiler}` was not found. \
             Install gfortran (macOS: `brew install gcc`, Debian/Ubuntu: \
             `sudo apt install gfortran`) and rebuild, or set CFAST_BIN."
        );
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let source_dir = manifest_dir.join(SOURCE_DIR);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    let object_dir = out_dir.join("cfast-obj");
    std::fs::create_dir_all(&object_dir).expect("create CFAST object directory");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let flags = upstream_flags(&target_os);
    let executable = out_dir.join(if target_os == "windows" {
        "cfast7.exe"
    } else {
        "cfast7"
    });

    let mut objects = Vec::with_capacity(SOURCES.len());
    for source in SOURCES {
        let object = object_dir.join(Path::new(source).with_extension("o"));
        let mut command = Command::new(&compiler);
        command
            .current_dir(&object_dir)
            .args(&flags)
            .arg("-J")
            .arg(&object_dir)
            .arg("-c")
            .arg(source_dir.join(source))
            .arg("-o")
            .arg(&object);
        run(&mut command, &format!("compile {source}"));
        objects.push(object);
    }

    let mut link = Command::new(&compiler);
    link.current_dir(&object_dir)
        .args(&flags)
        .arg("-o")
        .arg(&executable)
        .args(&objects);
    run(&mut link, "link CFAST");

    println!(
        "cargo:rustc-env=TFLOP_CFAST_BUNDLED_BIN={}",
        executable.display()
    );
}

/// Mirrors the `gnu_osx`, `gnu_linux`, and `gnu_win` targets in upstream's
/// `Build/CFAST/makefile`, plus the version macros its `GITINFOGNU` defines.
fn upstream_flags(target_os: &str) -> Vec<String> {
    let mut flags: Vec<String> = match target_os {
        "macos" => vec![
            "-O0",
            "-finit-local-zero",
            "-ffpe-trap=invalid,zero,overflow",
            "-ffpe-summary=none",
            "-fbacktrace",
        ],
        "windows" => vec![
            "-O0",
            "-finit-local-zero",
            "-ffpe-trap=invalid,zero,overflow",
            "-fbacktrace",
        ],
        _ => vec!["-O0"],
    }
    .into_iter()
    .map(str::to_owned)
    .collect();
    flags.extend([
        "-cpp".to_owned(),
        format!("-DGITHASH_PP=\"{CFAST_GIT_DESCRIBE}\""),
        format!("-DGITDATE_PP=\"{CFAST_GIT_DATE}\""),
        "-DBUILDDATE_PP=\"tflop cargo build\"".to_owned(),
    ]);
    flags
}

fn run(command: &mut Command, action: &str) {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("failed to {action}: {error}"));
    if !output.status.success() {
        panic!(
            "failed to {action} ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
