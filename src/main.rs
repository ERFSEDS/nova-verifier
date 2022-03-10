//! Does this show up on clap?
use anyhow::{Context, Result};
use clap::Parser;
use log::*;

/// Command line utility for converting toml config files to .ncf files for the Nova Flight Computer
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The path to the input configuration toml file
    #[clap(default_value_t = String::from("rocket.toml"))]
    input: String,

    /// The name of the output
    #[clap(default_value_t = String::from("config.ncf"))]
    output: String,
}

fn main() -> Result<()> {
    pretty_env_logger::init();
    let args = Args::parse();

    let input = args.input;
    info!("Reading input file {input}");
    let toml: String =
        std::fs::read_to_string(&input).with_context(|| format!("Reading file `{input}`"))?;

    let bytes =
        nova_verifier::verify(&toml).with_context(|| format!("Converting file `{input}`"))?;

    let output = args.output;
    info!("Writing converted configuration to {output}");
    std::fs::write(&output, bytes).with_context(|| format!("Writing file {output}"))?;

    Ok(())
}
