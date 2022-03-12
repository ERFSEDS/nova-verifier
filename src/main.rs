//! Does this show up on clap?
use anyhow::Result;
use clap::Parser;
use codemap_diagnostic::Diagnostic;
use log::*;
use nova_verifier::Session;

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

fn main() {
    match run() {
        Ok(warns) => {
            println!("Verify finished with {} warnings", warns.len());
        }
        Err(e) => {
            println!("Verify failed!");
        }
    }
}

fn run() -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    pretty_env_logger::init();
    let args = Args::parse();

    let input = args.input;
    info!("Reading input file {input}");

    let toml = std::fs::read_to_string(&input).unwrap();

    let mut session = Session::new();
    // We ignore the warnings since they are emitted to the user by default
    let (bytes, warns) = nova_verifier::verify(&mut session, toml, input)?;

    let output = args.output;
    info!("Writing converted configuration to {output}");
    std::fs::write(&output, bytes).unwrap();

    Ok(warns)
}
