//! `tflop-server`: serves one in-memory `World` over HTTP.
//!
//! Edits live only in memory; restarting the server resets the scene.

use std::{error::Error, fs, net::SocketAddr, path::PathBuf};

use clap::Parser;
use tflop_server::{
    DEMO_SCENE_JSON, DetectionOutcome, load_property, load_scene_with_detection,
    router_with_property,
};
use tflop_world_core::World;

#[derive(Debug, Parser)]
#[command(about = "Serve the tflop world API over HTTP")]
struct Args {
    /// Canonical scene JSON to load. Defaults to the built-in demo room.
    #[arg(long, env = "TFLOP_SCENE_PATH")]
    scene: Option<PathBuf>,

    /// Address to listen on.
    #[arg(long, env = "TFLOP_SERVER_ADDR", default_value = "127.0.0.1:3000")]
    addr: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let args = Args::parse();

    let (json, source) = match &args.scene {
        Some(path) => (
            fs::read_to_string(path)
                .map_err(|error| format!("cannot read scene {}: {error}", path.display()))?,
            path.display().to_string(),
        ),
        None => (DEMO_SCENE_JSON.to_owned(), "built-in demo room".to_owned()),
    };
    // Furniture found in the scan sits beside the scene as <id>.detected.json.
    let detection = args
        .scene
        .as_ref()
        .map(|path| path.with_extension("detected.json"))
        .and_then(|path| fs::read_to_string(&path).ok().map(|json| (path, json)));
    let (scene, outcome) =
        load_scene_with_detection(&json, detection.as_ref().map(|(_, json)| json.as_str()))
            .map_err(|error| format!("invalid scene ({source}): {error}"))?;
    if let Some((path, _)) = &detection {
        match outcome {
            DetectionOutcome::Merged(summary) => println!(
                "tflop-server: merged {} scanned objects from {} ({} skipped)",
                summary.merged,
                path.display(),
                summary.skipped
            ),
            DetectionOutcome::Ignored(reason) => {
                eprintln!("tflop-server: ignoring {}: {reason}", path.display());
            }
            DetectionOutcome::None => {}
        }
    }

    // The scan's CoStar record sits beside the scene as <id>.property.json.
    let property = args
        .scene
        .as_ref()
        .map(|path| path.with_extension("property.json"))
        .and_then(|path| fs::read_to_string(&path).ok().map(|json| (path, json)))
        .and_then(|(path, json)| match load_property(&json, &scene) {
            Ok(brief) => {
                let p = &brief.profile;
                println!(
                    "tflop-server: loaded CoStar {} {} from {}; fire runs default to {:?} floor, \
                     alarm noticed at {} kW / {} s, {:.0}% need help out ({} occupants suggested)",
                    brief.record.source.dataset,
                    brief.record.source.record_id,
                    path.display(),
                    p.floor_material.value,
                    p.alarm.value.notice_hrr_kw,
                    p.alarm.value.same_room_seconds,
                    p.injured_fraction.value * 100.0,
                    p.occupant_count.value
                );
                Some(brief)
            }
            Err(reason) => {
                eprintln!("tflop-server: ignoring {}: {reason}", path.display());
                None
            }
        });

    let listener = tokio::net::TcpListener::bind(args.addr).await?;
    println!("tflop-server: serving {source} on http://{}", args.addr);
    axum::serve(listener, router_with_property(World::new(scene), property)).await?;
    Ok(())
}
