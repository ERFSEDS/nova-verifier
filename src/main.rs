//! Does this show up on clap?
use anyhow::{Context, Result};
use clap::Parser;
use log::*;
use miette::{GraphicalReportHandler, GraphicalTheme, ReportHandler};
use nova_verifier::{MutipleErrors, SourceManager};
use std::{fmt::Formatter, rc::Arc};

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
        Ok(()) => {}
        Err(e) => {
            println!("{:?}", e);
            let mut out = String::new();
            GraphicalReportHandler::new_themed(GraphicalTheme::unicode())
                .render_report(&mut out, &e)
                .unwrap();
            print!("{out}");
        }
    }
}

fn run() -> Result<(), MutipleErrors> {
    pretty_env_logger::init();
    let args = Args::parse();

    let input = args.input;
    info!("Reading input file {input}");
    let manager = SourceManager::open(&input).unwrap();

    let bytes = nova_verifier::verify(Arc::clone(&manager))?;

    let output = args.output;
    info!("Writing converted configuration to {output}");
    std::fs::write(&output, bytes)
        .with_context(|| format!("Writing file {output}"))
        .unwrap();

    Ok(())
}
