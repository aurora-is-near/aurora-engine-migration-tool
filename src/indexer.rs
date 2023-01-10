use crate::rpc::{BlockKind, Client, IndexedData};
use borsh::{BorshDeserialize, BorshSerialize};
use near_primitives::types::BlockHeight;
use near_primitives::views::ChunkHeaderView;
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
    pub client: Client,
    pub data_file: PathBuf,
    pub last_saved_time: Instant,
    pub fetch_history: bool,
    pub force_index_from_block: Option<u64>,
}

impl Indexer {
    /// Init new indexer
    pub fn new<P: AsRef<Path>>(
        data_file: P,
        fetch_history: bool,
        block_height: Option<BlockHeight>,
    ) -> anyhow::Result<Self> {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let mut data = IndexerData::try_from_slice(&data).unwrap_or_default();

        if let Some(height) = block_height {
            data.last_block = height - 1;
        }

        Ok(Self {
            data: Arc::new(Mutex::new(data)),
            client: Client::new(),
            data_file: data_file.as_ref().to_path_buf(),
            last_saved_time: Instant::now(),
            fetch_history,
            force_index_from_block: block_height,
        })
    }

    #[cfg(test)]
    pub fn new_with_url<P: AsRef<Path>>(
        data_file: P,
        fetch_history: bool,
        block_height: Option<BlockHeight>,
        url: &str,
    ) -> anyhow::Result<Self> {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let mut data = IndexerData::try_from_slice(&data).unwrap_or_default();

        if let Some(height) = block_height {
            data.last_block = height - 1;
        }

        Ok(Self {
            data: Arc::new(Mutex::new(data)),
            client: Client::new_with_url(url),
            data_file: data_file.as_ref().to_path_buf(),
            last_saved_time: Instant::now(),
            fetch_history,
            force_index_from_block: block_height,
        })
    }

    pub fn stats(&self, extend: bool) {
        let data = self.data.lock().unwrap();
        println!("First block: {:?}", data.first_block);
        println!("Last block: {:?}", data.last_block);
        println!("Current block: {:?}", data.current_block);
        println!(
            "Missed block: [{:?}] {:?}",
            data.missed_blocks.len(),
            data.missed_blocks
        );
        println!("Accounts: {:?}", data.data.accounts.len());
        println!("Proofs: {:?}", data.data.proofs.len());

        if extend {
            println!("Log: {:#?}", data.data.logs);
        }
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
        let missed_blocks = self.data.lock().unwrap().missed_blocks.clone();
        self.client.set_missed_blocks(missed_blocks);
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
                block = self.client.get_block(BlockKind::Latest) => if let Ok(block) = block {
                    handle = self.handle_block(block).await;
                },
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

    #[cfg(test)]
    pub async fn run_n_blocks(&mut self, num_blocks: u64) -> anyhow::Result<()> {
        let missed_blocks = self.data.lock().unwrap().missed_blocks.clone();
        self.client.set_missed_blocks(missed_blocks);
        let last_block = self.data.lock().unwrap().last_block;
        println!("Starting height: {}", last_block);
        let mut handle = None;

        for _ in 0..num_blocks {
            if let Ok(block) = self.client.get_block(BlockKind::Latest).await {
                handle = self.handle_block(block).await;
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
    async fn handle_block(
        &mut self,
        block: (BlockHeight, Vec<ChunkHeaderView>),
    ) -> Option<tokio::task::JoinHandle<()>> {
        let last_block = self.data.lock().unwrap().last_block;
        // Skip, if block already exists
        if last_block >= block.0 {
            return None;
        }
        // Check, do we need fetch history data or force check from some block height
        let (height, chunks) = if self.force_index_from_block.is_some() || self.fetch_history {
            if block.0 - last_block > 0 {
                if let Ok(block) = self
                    .client
                    .get_block(BlockKind::Height(last_block + 1))
                    .await
                {
                    block
                } else {
                    // If block not found do not fail, just increment height
                    let mut data = self.data.lock().unwrap();
                    data.last_block = last_block + 1;
                    return None;
                }
            } else {
                block.clone()
            }
        } else {
            block.clone()
        };
        print!("\rHeight: {:?}", height);
        std::io::stdout().flush().expect("Flush failed");

        let indexed_data = self.client.get_chunk_indexed_data(chunks, height).await;
        self.set_indexed_data(
            height,
            indexed_data,
            self.client.unresolved_blocks.clone(),
            block.0,
        );

        // Save data
        if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
            self.last_saved_time = Instant::now();
            let current_block_height = block.0;
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
