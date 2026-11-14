//! CLI entry point for preparing a Matterport model for the tflop world engine.

use std::{
    collections::HashSet,
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use clap::Parser;
use tflop_matterport::MatterportClient;

#[derive(Debug, Parser)]
#[command(about = "Import a Matterport model into the canonical tflop scene format")]
struct Args {
    /// Matterport model ID (SID).
    #[arg(long, env = "MATTERPORT_MODEL_ID")]
    model_id: String,

    /// Destination JSON file. Defaults to data/prepared-scenes/<model-id>.json.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let args = Args::parse();

    let token_id = required_env("MATTERPORT_TOKEN_ID", "MATTERPORT_API_KEY")?;
    let token_secret = required_env("MATTERPORT_TOKEN_SECRET", "MATTERPORT_API_TOKEN")?;
    let output = args.output.unwrap_or_else(|| {
        Path::new("data")
            .join("prepared-scenes")
            .join(format!("{}.json", args.model_id))
    });

    let client = MatterportClient::new(token_id, token_secret);
    let scene = client.fetch_scene(&args.model_id).await?;
    let json = serde_json::to_string_pretty(&scene)?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, format!("{json}\n"))?;

    let rooms = scene
        .floors
        .iter()
        .map(|floor| floor.rooms.len())
        .sum::<usize>();
    let unique_edges = scene
        .floors
        .iter()
        .flat_map(|floor| &floor.rooms)
        .flat_map(|room| &room.boundary)
        .map(|edge| edge.id.as_str())
        .collect::<HashSet<_>>()
        .len();

    println!(
        "Imported {} floor(s), {rooms} room(s), and {unique_edges} unique boundary edge(s) to {}",
        scene.floors.len(),
        output.display()
    );

    Ok(())
}

fn required_env(primary: &str, compatibility_name: &str) -> Result<String, Box<dyn Error>> {
    for name in [primary, compatibility_name] {
        if let Ok(value) = env::var(name)
            && !value.trim().is_empty()
        {
            return Ok(value);
        }
    }

    Err(format!("missing {primary} in the environment or .env").into())
}
