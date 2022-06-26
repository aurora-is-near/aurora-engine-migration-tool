use crate::rpc::{BlockKind, RPC};
use borsh::{BorshDeserialize, BorshSerialize};
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_primitives::hash::CryptoHash;
use near_primitives::types::BlockHeight;
use near_primitives::views::ActionView;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct TxData {
    hash: CryptoHash,
    action: String,
    output: Vec<String>,
}

#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct BlockData {
    pub block_height: BlockHeight,
    pub transactions: Vec<TxData>,
}

#[derive(Debug, Default, BorshSerialize, BorshDeserialize)]
pub struct IndexerData {
    pub blocks: Vec<BlockData>,
    pub first_block: BlockHeight,
    pub last_block: BlockHeight,
    pub missed_blocks: HashSet<BlockHeight>,
    pub missed_txs: HashSet<CryptoHash>,
}

pub struct Indexer {
    pub data: IndexerData,
    pub data_file: PathBuf,
    pub last_saved_time: Instant,
    pub fetch_history: bool,
}

impl Indexer {
    pub fn new(data_file: PathBuf, get_history: bool) -> Self {
        let data = std::fs::read(&data_file).unwrap_or_default();
        let data: IndexerData = IndexerData::try_from_slice(&data[..]).unwrap_or_default();
        Self {
            data,
            data_file,
            last_saved_time: Instant::now(),
            fetch_history: get_history,
        }
    }

    async fn receiver(mut rx: tokio::sync::mpsc::Receiver<Vec<String>>) {
        while let Some(message) = rx.recv().await {
            println!("MSG = {:#?}", message);
        }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut rpc = RPC::new().await?;
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        tokio::spawn(async move {
            Self::receiver(rx).await;
        });

        let current_block = rpc.get_block(BlockKind::Latest).await?;
        let block = if self.fetch_history {
            if current_block.0 - self.data.last_block > 0 {
                rpc.get_block(BlockKind::Height(self.data.last_block + 1))
                    .await?
            } else {
                current_block
            }
        } else {
            current_block
        };
        let out = rpc.get_transactions_outcome(block.1).await;
        println!("Output: {:#?}", out);
        if !out.is_empty() {
            tokio::spawn(async move {
                tx.send(out).await.unwrap();
            });
        }

        Ok(())
    }

    pub async fn indexer(_history: bool) -> anyhow::Result<()> {
        let block_reference = near_primitives::types::BlockReference::Finality(
            near_primitives::types::Finality::Final,
        );

        // let client = JsonRpcClient::connect("https://rpc.mainnet.near.org");
        let client = JsonRpcClient::connect("https://archival-rpc.mainnet.near.org");

        let mut block_height_pool: HashSet<BlockHeight> = HashSet::new();
        let block = client
            .call(methods::block::RpcBlockRequest {
                block_reference: block_reference.clone(),
            })
            .await
            .expect("Failed get latest block");
        let final_height: u64 = block.header.height;
        let starting_height = 79364511; // 79372150  final_height;
        let mut height: u64 = starting_height;
        let block_limit = 10_000_000;

        println!("Starting height: {:?}", height);
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;

            print!("\rHeight: {:?}\r\n{:?}", height, starting_height - height);
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
                height -= 1;
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
                                            "\n\n{:?} [{:?}]: {:?}",
                                            height, chunk.shard_id, method_name,
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
                                            println!("Tx: {:#?}\n", tx_info.status);
                                        } else {
                                            println!("Failed get tx")
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
}
