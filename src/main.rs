use std::env::args;

pub mod indexer;
pub mod migration;
pub mod parser;
pub mod rpc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!(
        "Aurora Engine migration tool v{}",
        env!("CARGO_PKG_VERSION")
    );
    indexer::indexer().await?;

    let json_file = args().nth(1).expect("Expected json file");
    parser::parse(json_file);

    Ok(())
}
