use crate::rpc::{BlockKind, Client, IndexedData};
use borsh::{BorshDeserialize, BorshSerialize};
use near_primitives::types::BlockHeight;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
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
    pub force_blocks: bool,
}

impl Indexer {
    /// Init new indexer
    pub fn new<P: AsRef<Path>>(
        data_file: P,
        fetch_history: bool,
        block_height: Option<BlockHeight>,
        force_blocks: bool,
    ) -> anyhow::Result<Self> {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let mut data = IndexerData::try_from_slice(&data).unwrap_or_default();

        if let Some(height) = block_height {
            data.last_block = height - 1;
            if data.first_block > height {
                data.first_block = height;
            }
        }

        Ok(Self {
            data: Arc::new(Mutex::new(data)),
            data_file: data_file.as_ref().to_path_buf(),
            last_saved_time: Instant::now(),
            fetch_history,
            force_index_from_block: block_height,
            force_blocks,
        })
    }

    pub fn stats(&self, extend: bool) {
        let data = self.data.lock().unwrap();

        if extend {
            println!("Logs: {:#?}\n", data.data.logs);
            println!(
                "Missed block list: [{}] {:?}\n",
                data.missed_blocks.len(),
                data.missed_blocks
            );
        }
        println!(r#"First block: {:?}"#, data.first_block);
        println!("Last block: {:?}", data.last_block);
        println!("Current block: {:?}", data.current_block);
        println!("Missed blocks: {}", data.missed_blocks.len());
        println!("Accounts: {}", data.data.accounts.len());
        println!("Proofs: {}", data.data.proofs.len());
    }

    /// Save indexed data
    fn save_data<P: AsRef<Path>>(
        data: &IndexerData,
        data_file: P,
        current_block_height: BlockHeight,
    ) {
        std::fs::write(data_file, data.try_to_vec().expect("Failed serialize"))
            .expect("Failed save indexed data");
        println!("[SAVE: {:?}]", current_block_height);
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
        let mut client = Client::new();
        let missed_blocks = self.data.lock().unwrap().missed_blocks.clone();
        client.set_missed_blocks(missed_blocks);
        let last_block = self.data.lock().unwrap().last_block;
        println!("Starting height: {}", last_block);
        let mut handle = None;
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = tx.send(()).await;
        });

        loop {
            tokio::select! {
                h = self.handle_block(&mut client) => handle = h,
                _ = rx.recv() => break,
                else => break,
            }
        }

        // Wait for data saving
        if let Some(handle) = handle {
            handle.await.map_err(Into::into)
        } else {
            Ok(())
        }
    }

    /// Handle fetching blocks
    async fn handle_block(&mut self, client: &mut Client) -> Option<tokio::task::JoinHandle<()>> {
        let last_block = self.data.lock().unwrap().last_block;
        let (current_block, current_height) = if !self.force_blocks {
            if let Ok(block) = client.get_block(BlockKind::Latest).await {
                // Skip, if block already exists
                if last_block >= block.0 {
                    return None;
                }
                (Some(block.clone()), block.0)
            } else {
                (None, 0)
            }
        } else {
            (None, 0)
        };

        // Check, do we need fetch history data or force check from some block height
        let block = if self.force_index_from_block.is_some() || self.fetch_history {
            if let Ok(block) = client.get_block(BlockKind::Height(last_block + 1)).await {
                Some(block)
            } else {
                // If block not found do not fail, just increment height
                let mut data = self.data.lock().unwrap();
                data.last_block = last_block + 1;
                None
            }
        } else {
            current_block
        };
        let (height, chunks) = block?;

        print!("\rHeight: {:?}", height);
        std::io::stdout().flush().expect("Flush failed");

        let indexed_data = client.get_chunk_indexed_data(chunks, height).await;
        self.set_indexed_data(
            height,
            indexed_data,
            client.unresolved_blocks.clone(),
            current_height,
        );

        // Save data
        if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
            self.last_saved_time = Instant::now();
            let current_block_height = current_height;
            let data_file = self.data_file.clone();
            let data = self.data.lock().unwrap().clone();

            Some(tokio::spawn(async move {
                Self::save_data(&data, &data_file, current_block_height);
            }))
        } else {
            None
        }
    }
}
