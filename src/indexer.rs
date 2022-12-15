use crate::rpc::{BlockKind, IndexedData, RPC};
use borsh::{BorshDeserialize, BorshSerialize};
use near_primitives::types::BlockHeight;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

const SAVE_FILE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct IndexerData {
    pub first_block: BlockHeight,
    pub last_block: BlockHeight,
    pub current_block: BlockHeight,
    pub missed_blocks: HashSet<BlockHeight>,
    pub data: IndexedData,
}

pub struct Indexer {
    pub data: Arc<Mutex<IndexerData>>,
    pub data_file: PathBuf,
    pub last_saved_time: Instant,
    pub fetch_history: bool,
    pub force_index_from_block: Option<u64>,
}

impl Indexer {
    /// Init new indexer
    pub fn new(data_file: PathBuf, fetch_history: bool, block_height: Option<BlockHeight>) -> Self {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let mut data: IndexerData = IndexerData::try_from_slice(&data[..]).unwrap_or_default();

        if let Some(height) = block_height {
            data.last_block = height - 1;
        }
        Self {
            data: Arc::new(Mutex::new(data)),
            data_file,
            last_saved_time: Instant::now(),
            fetch_history,
            force_index_from_block: block_height,
        }
    }

    pub fn stats(&self, extend: bool) {
        let data = self.data.lock().unwrap();
        println!("First block: {:?}", data.first_block);
        println!("Last block: {:?}", data.last_block);
        println!("Current block: {:?}", data.current_block);
        println!("Missed block: {:?}", data.missed_blocks);
        println!("Accounts: {:?}", data.data.accounts.len());
        println!("Proofs: {:?}", data.data.proofs.len());
        if extend {
            println!("Log: {:#?}", data.data.logs);
        }
    }

    /// Save indexed data
    fn save_data(data: IndexerData, data_file: &PathBuf, current_block_height: BlockHeight) {
        std::fs::write(data_file, data.try_to_vec().expect("Failed serialize"))
            .expect("Failed save indexed data");
        println!(" [SAVE: {:?}]", current_block_height);
    }

    /// Set current index data
    pub fn set_indexed_data(
        &mut self,
        height: BlockHeight,
        indexed_data: IndexedData,
        missed_blocks: HashSet<BlockHeight>,
        current_block: BlockHeight,
    ) {
        let mut data = self.data.lock().unwrap();
        if data.first_block == 0 {
            data.first_block = height;
        }
        data.last_block = height;
        data.current_block = current_block;
        for account in indexed_data.accounts {
            data.data.accounts.insert(account);
        }
        for proof in indexed_data.proofs {
            data.data.proofs.insert(proof);
        }
        let mut logs = indexed_data.logs;
        data.data.logs.append(&mut logs);
        data.missed_blocks = missed_blocks;
    }

    /// Run indexing
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut rpc = RPC::new().await?;
        let missed_blocks = { self.data.lock().unwrap().missed_blocks.clone() };
        rpc.set_missed_blocks(missed_blocks);
        let last_block = { self.data.lock().unwrap().last_block };
        println!("Starting height: {:?}", last_block);

        loop {
            let current_block = rpc.get_block(BlockKind::Latest).await?;
            let last_block = { self.data.lock().unwrap().last_block };
            // Skip, if block already exists
            if last_block >= current_block.0 {
                continue;
            }
            // Check, do we need fetch history data or force check from some block height
            let (height, chunks) = if self.force_index_from_block.is_some() {
                rpc.get_block(BlockKind::Height(last_block + 1)).await?
            } else if self.fetch_history {
                if current_block.0 - last_block > 0 {
                    rpc.get_block(BlockKind::Height(last_block + 1)).await?
                } else {
                    current_block.clone()
                }
            } else {
                current_block.clone()
            };
            print!("\rHeight: {:?}", height);
            std::io::stdout().flush().expect("Flush failed");

            let indexed_data = rpc.get_chunk_indexed_data(chunks, height).await;
            self.set_indexed_data(
                height,
                indexed_data,
                rpc.unresolved_blocks.clone(),
                current_block.0,
            );

            // Save data
            if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
                self.last_saved_time = Instant::now();
                let current_block_height = current_block.0;
                let data_file = self.data_file.clone();
                let data = self.data.lock().unwrap().clone();
                tokio::spawn(async move {
                    Self::save_data(data, &data_file, current_block_height);
                });
            }
        }
    }
}
