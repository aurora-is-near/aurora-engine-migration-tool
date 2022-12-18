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
                )
                .arg(
                    arg!(-s --stat "Show short indexing statistic")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    arg!(--fullstat "Show full indexing statistic")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    arg!(-b --block <BLOCK_HEIGHT> "Start indexing from specific block")
                        .value_parser(value_parser!(u64)),
                ),
        )
        .subcommand(
            Command::new("migrate-indexed")
                .about("migrate indexed data")
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
            let stat = cmd.get_flag("stat");
            let fullstat = cmd.get_flag("fullstat");
            let block = cmd.get_one::<u64>("block").copied();
            let mut indexer = Indexer::new("data.borsh".into(), history, block);
            if stat {
                indexer.stats(false);
            } else if fullstat {
                indexer.stats(true);
            } else {
                indexer.run().await?
            }
        }
        Some(("migrate", cmd)) => {
            let data_file = cmd.get_one::<PathBuf>("file").expect("Expected data file");

            let account_id = cmd
                .get_one::<String>("account")
                .expect("Expected account-id");
            let account_key = cmd.get_one::<String>("key").expect("Expected account-key");

            Migration::new(data_file, account_id.clone(), account_key.clone())
                .await?
                .run()
                .await?;
        }
        Some(("migrate-indexed", _cmd)) => {
            println!("migrate-indexed");
        }
        _ => (),
    }

    Ok(())
}
