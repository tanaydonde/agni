//! Detects furniture in an imported scene by comparing its furnished and
//! defurnished meshes, and writes the boxes beside the scene.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use clap::Parser;
use tflop_furniture_detect::{DetectParams, FloorMask, debug_masks, detect, parse_obj};
use tflop_world_core::Scene;

#[derive(Debug, Parser)]
#[command(about = "Detect furniture boxes from a scene's furnished and defurnished meshes")]
struct Args {
    /// Matterport model ID (SID). Inputs and output default to
    /// data/prepared-scenes/<model-id>.{json,obj,furnished.obj,detected.json}.
    #[arg(long, env = "MATTERPORT_MODEL_ID")]
    model_id: String,

    /// Directory holding the prepared scene files.
    #[arg(long, default_value = "data/prepared-scenes")]
    dir: PathBuf,

    /// Voxel and grid cell size in metres.
    #[arg(long)]
    voxel: Option<f64>,

    /// Distance from a defurnished surface within which furnished surfaces
    /// count as room shell, in metres.
    #[arg(long)]
    shell_tolerance: Option<f64>,

    /// Also write the top-down furniture grid of each floor as a PGM image
    /// (north up): black cells went into boxes, grey cells had furniture
    /// voxels too sparse to count. Use for tuning.
    #[arg(long, value_name = "FILE.pgm")]
    debug_mask: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let args = Args::parse();
    let file = |suffix: &str| args.dir.join(format!("{}{suffix}", args.model_id));

    let scene: Scene =
        serde_json::from_str(&read(&file(".json"), "scene", "cargo run -p import-scene")?)?;
    let defurnished = parse_obj(&read(
        &file(".obj"),
        "defurnished mesh",
        "cargo run -p import-scene",
    )?)?;
    let furnished = parse_obj(&read(
        &file(".furnished.obj"),
        "furnished mesh",
        "cargo run -p import-scene -- --furnished-mesh",
    )?)?;

    let mut params = DetectParams::default();
    if let Some(voxel) = args.voxel {
        params.voxel_m = voxel;
    }
    if let Some(tolerance) = args.shell_tolerance {
        params.shell_tolerance_m = tolerance;
    }

    let started = Instant::now();
    let detection = detect(&scene, &furnished, &defurnished, params);
    let elapsed = started.elapsed();

    if let Some(path) = &args.debug_mask {
        for (index, mask) in debug_masks(&scene, &furnished, &defurnished, params)
            .iter()
            .enumerate()
        {
            let path = if index == 0 {
                path.clone()
            } else {
                path.with_extension(format!("{index}.pgm"))
            };
            fs::write(&path, mask_pgm(mask))?;
            println!(
                "Wrote the mask of floor {} to {} (origin {:.3}, {:.3}; {} m cells)",
                mask.floor_id,
                path.display(),
                mask.origin_m[0],
                mask.origin_m[1],
                mask.cell_m
            );
        }
    }

    let output = file(".detected.json");
    fs::write(
        &output,
        format!("{}\n", serde_json::to_string_pretty(&detection)?),
    )?;

    println!(
        "{:<13} {:<14} {:<9} {:<22} {:>6} {:>6} {:>6} {:>6}",
        "id", "room", "label", "catalog asset", "w", "d", "h", "yaw"
    );
    for object in &detection.objects {
        println!(
            "{:<13} {:<14} {:<9} {:<22} {:>6.2} {:>6.2} {:>6.2} {:>6.0}",
            object.id,
            object.room_type.as_deref().unwrap_or("-"),
            format!("{:?}", object.label).to_lowercase(),
            object.asset_id.as_deref().unwrap_or("-"),
            object.size_m.width_m,
            object.size_m.depth_m,
            object.size_m.height_m,
            object.yaw_degrees,
        );
    }
    println!(
        "Detected {} object(s) in {:.1}s; wrote {}",
        detection.objects.len(),
        elapsed.as_secs_f64(),
        output.display()
    );
    Ok(())
}

fn read(path: &Path, what: &str, how: &str) -> Result<String, Box<dyn Error>> {
    fs::read_to_string(path).map_err(|error| {
        format!(
            "couldn't read the {what} at {} ({error}); create it with `{how}`",
            path.display()
        )
        .into()
    })
}

/// Binary PGM, one pixel per cell, with north (+Y) at the top.
fn mask_pgm(mask: &FloorMask) -> Vec<u8> {
    let mut image = format!("P5\n{} {}\n255\n", mask.nx, mask.ny).into_bytes();
    for row in (0..mask.ny).rev() {
        for column in 0..mask.nx {
            let cell = row * mask.nx + column;
            image.push(if mask.occupied[cell] {
                0
            } else if mask.counts[cell] > 0 {
                160
            } else {
                255
            });
        }
    }
    image
}
