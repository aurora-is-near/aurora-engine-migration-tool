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
            Command::new("parse")
                .about("Parse Aurora Engine contract state snapshot and store result to file serialized with borsh")
                .arg(
                    arg!(-f --file <FILE> "Aurora Engine snapshot json file")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-o --output <FILE> "Output file with results data serialized with borsh")
                        .required(false)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("indexer")
                .about("Run indexing NEAR blockchain blocks and chunks for all shards, for specific NEAR network. For Aurora Engine contract.")
                .arg(
                    arg!(-s --stat "Show short indexed statistic")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    arg!(--fullstat "Show full indexed statistic")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    arg!(-b --block <BLOCK_HEIGHT> "Start indexing from specific block")
                        .required(true)
                        .value_parser(value_parser!(u64)),
                ),
        )
        .subcommand(
            Command::new("prepare-migrate-indexed")
                .about("Prepare indexed data for migration. Should be invoked before migration")
                .arg(
                    arg!(-f --file <FILE> "File with parsed or indexed data serialized with borsh")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-o --output <FILE> "Output file with migration results data serialized with borsh")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("migrate")
                .about("migrate Aurora contract NEP-141 state")
                .arg(
                    arg!(-f --file <FILE> "Prepared state file for migration")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-c --contract <ACCOUNT_ID> "Account ID of aurora-eth-connector")
                        .required(true),
                )
                .arg(
                    arg!(-s --signer <ACCOUNT_ID> "Signer Account ID")
                        .required(true),
                )
                .arg(
                    arg!(-k --key <ACCOUNT_KEY> "Account private key for sign migration transactions")
                        .required(true),
                )
        )
        .subcommand(
            Command::new("combine-indexed-and-state-data")
                .about("Combine indexed and state data")
                .arg(
                    arg!(--state <FILE> "Path to the state data file in borsh format")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(--indexed <FILE> "Path to the indexed data file in borsh format")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(--output <FILE> "Output file for combined state and indexed data in borsh format")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
        )
        .subcommand(
            Command::new("check-migration")
                .about("Check migration correctness")
                .arg(
                    arg!(-f --file <FILE> "Prepared state file for migration")
                        .required(true)
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-c --contract <ACCOUNT_ID> "Account ID of aurora-eth-connector")
                        .required(true),
                )
                .arg(
                arg!(-s --signer <ACCOUNT_ID> "Signer Account ID")
                    .required(true),
                )
                .arg(
                    arg!(-k --key <ACCOUNT_KEY> "Account private key for sign transactions")
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
                .ok_or_else(|| anyhow::anyhow!("Expected snapshot file"))?;
            let output = cmd.get_one::<PathBuf>("output");
            parser::parse(snapshot_json_file, output)?;
        }
        Some(("indexer", cmd)) => {
            let stat = cmd.get_flag("stat");
            let fullstat = cmd.get_flag("fullstat");
            let block = cmd
                .get_one::<u64>("block")
                .copied()
                .expect("Expected start block height");
            let mut indexer = Indexer::new("data.borsh", block)?;

            if stat {
                indexer.stats(false).await;
            } else if fullstat {
                indexer.stats(true).await;
            } else {
                indexer.run().await?;
            }
        }
        Some(("migrate", cmd)) => {
            let data_file = cmd.get_one::<PathBuf>("file").expect("Expected data file");

            let contract_account_id = cmd
                .get_one::<String>("contract")
                .expect("Expected account-id");
            let signer_account_id = cmd
                .get_one::<String>("signer")
                .expect("Expected account-id");
            let signer_account_key = cmd.get_one::<String>("key").expect("Expected account-key");

            Migration::new(
                data_file,
                contract_account_id.clone(),
                signer_account_id.clone(),
                signer_account_key.clone(),
            )?
            .run()
            .await?;
        }
        Some(("prepare-migrate-indexed", cmd)) => {
            let input_data_file = cmd.get_one::<PathBuf>("file").expect("Expected data file");
            let output_file = cmd
                .get_one::<PathBuf>("output")
                .expect("Expected output file");
            Migration::prepare_indexed(input_data_file, output_file).await?;
        }
        Some(("combine-indexed-and-state-data", cmd)) => {
            let state_data_file = cmd.get_one::<PathBuf>("state").expect("Expected data file");
            let indexed_data_file = cmd
                .get_one::<PathBuf>("indexed")
                .expect("Expected data file");
            let output_file = cmd
                .get_one::<PathBuf>("output")
                .expect("Expected output file");
            Migration::combine_indexed_and_state_data(
                state_data_file,
                indexed_data_file,
                output_file,
            )?;
        }
        Some(("check-migration", cmd)) => {
            let data_file = cmd.get_one::<PathBuf>("file").expect("Expected data file");

            let contract_account_id = cmd
                .get_one::<String>("contract")
                .expect("Expected account-id");
            let signer_account_id = cmd
                .get_one::<String>("signer")
                .expect("Expected account-id");
            let signer_account_key = cmd.get_one::<String>("key").expect("Expected account-key");

            Migration::new(
                data_file,
                contract_account_id.clone(),
                signer_account_id.clone(),
                signer_account_key.clone(),
            )?
            .validate_migration()
            .await?;
        }
        _ => (),
    }

    Ok(())
}
