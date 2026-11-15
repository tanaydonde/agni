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
    /// The defurnished mesh is written beside it as <model-id>.obj.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Skip downloading Matterport's defurnished mesh.
    #[arg(long)]
    skip_mesh: bool,

    /// Also download the furnished mesh from the MatterPak (about 140 MB) to
    /// <model-id>.furnished.obj, for `cargo run -p detect-furniture`.
    #[arg(long)]
    furnished_mesh: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let args = Args::parse();

    let token_id = required_env(&[
        "MATTERPORT_TOKEN_ID",
        "MATTERPORT_API_KEY",
        "COSTAR_MATTERPORT_KEY",
    ])?;
    let token_secret = required_env(&[
        "MATTERPORT_TOKEN_SECRET",
        "MATTERPORT_API_TOKEN",
        "COSTAR_MATTERPORT_PASSWORD",
    ])?;
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

    if !args.skip_mesh {
        let mesh_path = output.with_extension("obj");
        match client.fetch_defurnished_mesh(&args.model_id).await? {
            Some(obj) => {
                fs::write(&mesh_path, obj)?;
                println!("Saved defurnished mesh to {}", mesh_path.display());
            }
            None => println!("No defurnished mesh is available for {}", args.model_id),
        }
    }

    if args.furnished_mesh {
        let mesh_path = output.with_extension("furnished.obj");
        println!("Downloading the MatterPak for the furnished mesh…");
        let obj = client.fetch_furnished_mesh(&args.model_id).await?;
        fs::write(&mesh_path, obj)?;
        println!("Saved furnished mesh to {}", mesh_path.display());
    }

    Ok(())
}

/// Reads the first non-empty variable among `names`; later names are
/// compatibility aliases (including the CoStar hackathon key names).
fn required_env(names: &[&str]) -> Result<String, Box<dyn Error>> {
    for &name in names {
        if let Ok(value) = env::var(name)
            && !value.trim().is_empty()
        {
            return Ok(value);
        }
    }

    Err(format!("missing {} in the environment or .env", names[0]).into())
}
