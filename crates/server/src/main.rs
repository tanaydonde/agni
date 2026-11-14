//! `tflop-server`: serves one in-memory `World` over HTTP.
//!
//! Edits live only in memory; restarting the server resets the scene.

use std::{error::Error, fs, net::SocketAddr, path::PathBuf};

use clap::Parser;
use tflop_server::{DEMO_SCENE_JSON, load_scene, router};
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
    let scene = load_scene(&json).map_err(|error| format!("invalid scene ({source}): {error}"))?;

    let listener = tokio::net::TcpListener::bind(args.addr).await?;
    println!("tflop-server: serving {source} on http://{}", args.addr);
    axum::serve(listener, router(World::new(scene))).await?;
    Ok(())
}
