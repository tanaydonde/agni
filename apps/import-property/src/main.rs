//! Joins a Matterport model to its CoStar record and writes it beside the
//! prepared scene as `<model-id>.property.json`, where the world server
//! picks it up. The server never reads the CoStar kit itself.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use clap::Parser;

#[derive(Debug, Parser)]
#[command(about = "Join a Matterport model to its CoStar property record")]
struct Args {
    /// Matterport model ID (SID).
    #[arg(long, env = "MATTERPORT_MODEL_ID")]
    model_id: String,

    /// Unzipped CoStar hackathon kit (holds commercial-properties.csv).
    #[arg(long, env = "COSTAR_DATA_PATH", default_value = "data/costar-kit")]
    kit: PathBuf,

    /// Destination. Defaults to data/prepared-scenes/<model-id>.property.json.
    #[arg(long)]
    output: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let args = Args::parse();
    let output = args.output.unwrap_or_else(|| {
        Path::new("data")
            .join("prepared-scenes")
            .join(format!("{}.property.json", args.model_id))
    });

    let record = tflop_costar::find_by_model_id(&args.kit, &args.model_id)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &output,
        format!("{}\n", serde_json::to_string_pretty(&record)?),
    )?;

    println!(
        "Joined {} to CoStar {} {} ({}): {} facts, {} tenants, {} conflicts -> {}",
        args.model_id,
        record.source.dataset,
        record.source.record_id,
        record.address.as_deref().unwrap_or("no address"),
        record.facts.len(),
        record.tenants.len(),
        record.conflicts.len(),
        output.display()
    );
    Ok(())
}
