use crate::indexer::Indexer;
use crate::migration::Migration;
use clap::{arg, command, value_parser, ArgAction, Command};
use std::path::PathBuf;

pub mod indexer;
mod migration;
mod parser;
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
        .subcommand(
            Command::new("migrate")
                .about("migrate Aurora contract NEP-141 state")
                .arg(
                    arg!(-f --file <FILE> "prepared state file for migration")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-a --account <ACCOUNT_ID> "Account ID to run migration")
                        .required(true),
                )
                .arg(
                    arg!(-k --key <ACCOUNT_KEY> "Account private key for sign migration transactions")
                        .required(true),
                )
                .arg(
                    arg!(-c --contract <CONTRACT> "Contract to migrate data")
                        .required(true),
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
            let history = cmd.get_flag("history");
            Indexer::new("data.borsh".into(), history).run().await?;
            // indexer::indexer(history).await?;
        }
        Some(("migrate", cmd)) => {
            let data_file = cmd.get_one::<PathBuf>("file").expect("Expected data file");

            let account_id = cmd
                .get_one::<String>("account")
                .expect("Expected account-id");
            let account_key = cmd.get_one::<String>("key").expect("Expected account-key");
            let contract = cmd
                .get_one::<String>("contract")
                .expect("Expected contract");
            Migration::new(
                data_file,
                account_id.clone(),
                account_key.clone(),
                contract.clone(),
            )
            .await?
            .run()
            .await?;
        }
        _ => (),
    }

    Ok(())
}
