use crate::rpc::{BlockKind, RPC};
use borsh::{BorshDeserialize, BorshSerialize};
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_primitives::hash::CryptoHash;
use near_primitives::types::BlockHeight;
use near_primitives::views::ActionView;
use near_sdk::AccountId;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

const SAVE_FILE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct TxData {
    hash: CryptoHash,
    action: String,
    output: Vec<String>,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct BlockData {
    pub block_height: BlockHeight,
    pub transactions: Vec<TxData>,
}

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
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
    pub fn new(data_file: PathBuf, fetch_history: bool) -> Self {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let data: IndexerData = IndexerData::try_from_slice(&data[..]).unwrap_or_default();
        Self {
            data,
            data_file,
            last_saved_time: Instant::now(),
            fetch_history,
        }
    }

    fn save_data(_data: Arc<Mutex<IndexerData>>) {}

    pub fn set_indexed_data(
        data: Arc<Mutex<IndexerData>>,
        height: BlockHeight,
        _output: Vec<AccountId>,
    ) {
        let mut data = data.lock().unwrap();
        data.last_block = height;
        if data.first_block == 0 {
            data.first_block = height;
        }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut rpc = RPC::new().await?;
        let data = Arc::new(Mutex::new(self.data.clone()));
        loop {
            let current_block = rpc.get_block(BlockKind::Latest).await?;
            // Skip, if block already exists
            if self.data.last_block == current_block.0 {
                continue;
            }
            // Check, do we need fetch history data
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
            print!("\rHeight: {:?}", block.0);
            std::io::stdout().flush().expect("Flush failed");

            let out = rpc.get_transactions_outcome(block.1).await;

            {
                let data = data.clone();
                Self::set_indexed_data(data, block.0, out);
            }

            // Save data
            if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
                self.last_saved_time = Instant::now();
                let data = data.clone();
                tokio::spawn(async {
                    Self::save_data(data);
                });
            }
        }
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
