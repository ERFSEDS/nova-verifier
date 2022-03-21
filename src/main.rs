//! Does this show up on clap?
use anyhow::Result;
use clap::Parser;
use codemap_diagnostic::Diagnostic;
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

fn main() {
    match run() {
        Ok(d) => {
            info!("Verify finished with {} diagnostics", d.len());
        }
        Err(d) => {
            info!("Verify failed with {} diagnostics!", d.len());
        }
    }
}

fn run() -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    pretty_env_logger::init();
    let args = Args::parse();

    let src_path = args.input;
    let dst_path = args.output;

    let r = nova_verifier::verify_file(src_path, dst_path.clone());

    let bytes = std::fs::read(dst_path).unwrap();
    let obj: nova_software_common::index::ConfigFile = postcard::from_bytes(&bytes).unwrap();
    if r.is_ok() {
        trace!("Encoded obj is: {obj:#?}");
    }

    r
}
