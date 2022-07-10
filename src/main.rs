use near_primitives::types::BlockHeight;
use near_primitives::views::ActionView;
use std::collections::HashSet;
use std::env::args;
use std::io::Write;
use std::time::Duration;

pub mod migration;
pub mod parser;
pub mod rpc;

async fn indexer() -> anyhow::Result<()> {
    use near_jsonrpc_client::{methods, JsonRpcClient};

    let block_reference =
        near_primitives::types::BlockReference::Finality(near_primitives::types::Finality::Final);

    let client = JsonRpcClient::connect("https://rpc.mainnet.near.org");
    let mut block_height_pool: HashSet<BlockHeight> = HashSet::new();
    let block = client
        .call(methods::block::RpcBlockRequest {
            block_reference: block_reference.clone(),
        })
        .await
        .expect("Failed get latest block");
    let final_height: u64 = block.header.height;
    let mut height: u64 = 79372150; // final_height;
    let block_limit = 10_000_000;

    println!("Starting height: {:?}", height);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;

        print!("\rHeight: {:?}", height);
        std::io::stdout().flush().expect("Flush failed");

        let block_reference = near_primitives::types::BlockReference::BlockId(
            near_primitives::types::BlockId::Height(height),
        );
        let chunks = if let Ok(block_res) = client
            .call(methods::block::RpcBlockRequest { block_reference })
            .await
        {
            block_res.chunks
        } else {
            println!("\nFailed get block: {:?}", height);
            continue;
        };
        for chunk in chunks {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(chunk_res) = client
                .call(methods::chunk::RpcChunkRequest {
                    chunk_reference:
                        near_jsonrpc_primitives::types::chunks::ChunkReference::ChunkHash {
                            chunk_id: chunk.chunk_hash,
                        },
                })
                .await
            {
                if !chunk_res.transactions.is_empty() {
                    for tx in &chunk_res.transactions {
                        if tx.receiver_id.as_str() == "aurora" {
                            for action in &tx.actions {
                                if let ActionView::FunctionCall { method_name, .. } = action {
                                    if method_name == "submit" {
                                        continue;
                                    }
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    println!(
                                        "\n{:?} [{:?}]: {:?}",
                                        height, chunk.shard_id, method_name
                                    );
                                    if let Ok(tx_info) = client
                                        .call(methods::tx::RpcTransactionStatusRequest {
                                            transaction_info:
                                                methods::tx::TransactionInfo::TransactionId {
                                                    hash: tx.hash,
                                                    account_id: tx.signer_id.clone(),
                                                },
                                        })
                                        .await
                                    {
                                        println!("\nTx: {:#?}", tx_info.status);
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                println!("\nFailed get chunk for height: {:?}", height);
                block_height_pool.insert(height);
            }
        }
        height -= 1;
        if height <= final_height - block_limit {
            break;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!(
        "Aurora Engine migration tool v{}",
        env!("CARGO_PKG_VERSION")
    );
    indexer().await?;

    let json_file = args().nth(1).expect("Expected json file");
    parser::parse(json_file);

    Ok(())
}
