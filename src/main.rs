use clap::{arg, command, value_parser, ArgAction, Command};
use std::path::PathBuf;

mod indexer;
pub mod migration;
pub mod parser;
pub mod rpc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = command!()
        .subcommand_required(true)
        .subcommand(
            Command::new("parser")
                .about("parse Aurora state snapshot")
                .arg(
                    arg!(-f --file <FILE> "Snapshot json file")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-o --output <FILE> "Output file with results data")
                        .required(false)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("indexer")
                .about("run indexing NEAR blockchain blocks for Aurora contract")
                .arg(
                    arg!(-H --history "Indexing missed historical blocks")
                        .action(ArgAction::SetTrue),
                ),
        )
        .get_matches();

    println!(
        "Aurora Engine migration tool v{}",
        env!("CARGO_PKG_VERSION")
    );

    match matches.subcommand() {
        Some(("parse", cmd)) => {
            let snapshot_json_file = cmd
                .get_one::<PathBuf>("file")
                .expect("Expected snapshot file");
            let output = cmd.get_one::<PathBuf>("output");
            parser::parse(snapshot_json_file, output.cloned());
        }
        Some(("indexer", cmd)) => {
            let _history = cmd.get_one::<bool>("history");
            indexer::indexer().await?;
        }
        _ => (),
    }

    Ok(())
}
