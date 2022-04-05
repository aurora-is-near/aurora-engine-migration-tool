use crate::rpc::{BlockKind, IndexedData, RPC};
use borsh::{BorshDeserialize, BorshSerialize};
use near_primitives::types::{AccountId, BlockHeight};
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
    pub fn new(data_file: PathBuf, fetch_history: bool, block: Option<u64>) -> Self {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let mut data: IndexerData = IndexerData::try_from_slice(&data[..]).unwrap_or_default();
        if let Some(height) = block {
            data.last_block = height - 1;
        }
        Self {
            data: Arc::new(Mutex::new(data)),
            data_file,
            last_saved_time: Instant::now(),
            fetch_history,
            force_index_from_block: block,
        }
    }

    /// Save indexed data
    fn save_data(
        data: Arc<Mutex<IndexerData>>,
        height: BlockHeight,
        _accounts: HashSet<AccountId>,
        _proofs: HashSet<String>,
    ) {
        let mut data = data.lock().unwrap();
        data.last_block = height;

        println!(" save_data");
    }

    /// Set current index data
    pub fn set_indexed_data(
        &mut self,
        height: BlockHeight,
        indexed_data: IndexedData,
        missed_blocks: HashSet<BlockHeight>,
    ) {
        let mut data = self.data.lock().unwrap();
        if data.first_block == 0 {
            data.first_block = height;
        }
        data.last_block = height;
        data.data = indexed_data;
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
            let block = if self.force_index_from_block.is_some() {
                rpc.get_block(BlockKind::Height(last_block + 1)).await?
            } else if self.fetch_history {
                if current_block.0 - last_block > 0 {
                    rpc.get_block(BlockKind::Height(last_block + 1)).await?
                } else {
                    current_block
                }
            } else {
                current_block
            };
            print!("\rHeight: {:?}", block.0);
            std::io::stdout().flush().expect("Flush failed");

            let indexed_data = rpc.get_chunk_indexed_data(block.1, block.0).await;
            self.set_indexed_data(block.0, indexed_data, rpc.unresolved_blocks.clone());

            // Save data
            if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
                self.last_saved_time = Instant::now();
                //let data = data.clone();
                tokio::spawn(async move {
                    //Self::save_data(data, block.0, out.0, out.1);
                });
            }
        }
    }
}
