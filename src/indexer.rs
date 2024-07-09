use crate::rpc::{BlockKind, Client, IndexedData};
use near_primitives::hash::CryptoHash;
use near_primitives::types::BlockHeight;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::signal::unix::SignalKind;
use tokio::time::{sleep, Instant};

const SAVE_FILE_TIMEOUT: Duration = Duration::from_secs(60);
const FORWARD_BLOCK_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct IndexerData {
    pub first_block: BlockHeight,
    pub last_block: BlockHeight,
    pub last_handled_block: BlockHeight,
    pub current_block: BlockHeight,
    pub last_block_hash: Option<CryptoHash>,
    pub missed_blocks: HashSet<BlockHeight>,
    pub data: IndexedData,
}

pub struct Indexer {
    pub data: Arc<Mutex<IndexerData>>,
    pub data_file: PathBuf,
    pub fetch_history: bool,
    pub force_index_from_block: Option<u64>,
    pub force_blocks: bool,
    forward_block: Option<u64>,
    last_saved_time: Instant,
    last_forward_time: Instant,
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
            fetch_history,
            force_index_from_block: block_height,
            force_blocks,
            forward_block: None,
            last_saved_time: Instant::now(),
            last_forward_time: Instant::now(),
        })
    }

    pub async fn stats(&self, extend: bool) {
        let mut client = Client::new();
        let height = if let Ok(block) = client.get_block(BlockKind::Latest).await {
            block.0
        } else {
            0
        };
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
        println!("Last saved in current block: {:?}", data.current_block);
        println!("Current block: {height:?}");
        println!("Missed blocks: {}", data.missed_blocks.len());
        println!("Accounts: {}", data.data.accounts.len());
        println!("Proofs: {}", data.data.proofs.len());
    }

    /// Save indexed data
    fn save_data<P: AsRef<Path>>(
        data: &IndexerData,
        data_file: P,
        current_block_height: BlockHeight,
        first_handled_block_height: BlockHeight,
        last_handled_block_height: BlockHeight,
    ) {
        std::fs::write(data_file, data.try_to_vec().expect("Failed serialize"))
            .expect("Failed save indexed data");
        println!(
            " [SAVE: current block: {current_block_height:?}, \
                          first handled block: {first_handled_block_height:?}, \
                          last handled block: {last_handled_block_height:?}]"
        );
    }

    /// Set current index data
    pub fn set_indexed_data(
        &mut self,
        height: BlockHeight,
        indexed_data: IndexedData,
        missed_blocks: HashSet<BlockHeight>,
        current_block: BlockHeight,
        last_block: BlockHeight,
        first_block: BlockHeight,
        block_hash: CryptoHash,
    ) {
        let mut data = self.data.lock().unwrap();
        data.first_block = first_block;
        if data.first_block == 0 {
            data.first_block = height;
        }
        data.last_block = last_block;
        data.last_handled_block = last_block;
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
        data.last_block_hash = Some(block_hash);
    }

    fn shutdown_listener() -> tokio::sync::mpsc::Receiver<()> {
        use tokio::signal;
        async fn send_msg(tx: tokio::sync::mpsc::Sender<()>) {
            println!("\n[Waiting shutdown]");
            let _ = tx.send(()).await;
        }

        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let mut terminate = signal::unix::signal(SignalKind::terminate()).unwrap();
        let mut interrupt = signal::unix::signal(SignalKind::interrupt()).unwrap();
        let mut quit = signal::unix::signal(SignalKind::quit()).unwrap();
        let mut tstp = signal::unix::signal(SignalKind::from_raw(libc::SIGTSTP)).unwrap();

        tokio::spawn(async move {
            tokio::select! {
                _ = signal::ctrl_c() => send_msg(tx).await,
                _ = terminate.recv() => send_msg(tx).await,
                _ = interrupt.recv() => send_msg(tx).await,
                _ = quit.recv() => send_msg(tx).await,
                _ = tstp.recv() => send_msg(tx).await,
            }
        });
        rx
    }

    /// Run indexing
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut client = Client::new();
        let missed_blocks = self.data.lock().unwrap().missed_blocks.clone();
        client.set_missed_blocks(missed_blocks);
        let last_block = self.data.lock().unwrap().last_block;
        println!("Starting height: {last_block}");
        let mut handle = None;

        let mut shutdown_stream = Self::shutdown_listener();
        loop {
            tokio::select! {
                h = self.handle_block(&mut client) => handle = h,
                _ = shutdown_stream.recv() => break,
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
        let first_block = self.data.lock().unwrap().first_block;
        let (current_block, current_height) = if self.force_blocks {
            (None, 0)
        } else if self.forward_block == None
            || self.last_forward_time.elapsed() > FORWARD_BLOCK_TIMEOUT
        {
            self.last_forward_time = Instant::now();
            if let Ok(block) = client.get_block(BlockKind::Latest).await {
                // Skip, if block already exists
                if last_block > block.0 {
                    println!(
                        "ERROR: last handled block cannot be bigger latest block. \
                              Sleep: {FORWARD_BLOCK_TIMEOUT:?}"
                    );
                    sleep(FORWARD_BLOCK_TIMEOUT).await;
                    return None;
                }
                self.forward_block = Some(block.0);
                (Some(block.clone()), block.0)
            } else {
                (None, 0)
            }
        } else {
            (None, 0)
        };

        // Calculate Height to catch it. Depend on several parameters.
        // 1. if it's just `index_from_block` - just linear increment `last_block`
        // 2. for other cases cha
        //   2.1. if `last_block > forward_block` - fetch history
        //   2.2. if 2.1. false - fetch forward blocks
        //   2.3. if `forward_block` - just fetch history
        let (num_height, last_block, first_block) = if self.force_index_from_block.is_some() {
            (last_block + 1, last_block + 1, first_block)
        } else if let Some(forward_block) = self.forward_block {
            if forward_block <= last_block + 1 {
                self.forward_block = None;
                (first_block - 1, last_block, first_block - 1)
            } else {
                (last_block + 1, last_block + 1, first_block)
            }
        } else {
            (first_block - 1, last_block, first_block - 1)
        };

        // Get `current_height`
        let current_height = if current_height == 0 {
            if let Some(height) = self.forward_block {
                height
            } else {
                0
            }
        } else {
            current_height
        };

        // Check, do we need fetch history data or force check from some block height
        let block = if self.force_index_from_block.is_some() || self.fetch_history {
            if num_height > current_height {
                println!(
                    "Try to fetch block with height bigger than latest block. \
                          Sleep: {FORWARD_BLOCK_TIMEOUT:?}"
                );
                sleep(FORWARD_BLOCK_TIMEOUT).await;
                None
            } else if let Ok(block) = client.get_block(BlockKind::Height(num_height)).await {
                Some(block)
            } else {
                // If block not found do not fail, just increment height
                let mut data = self.data.lock().unwrap();
                data.last_block = last_block;
                None
            }
        } else {
            current_block
        };
        let (height, chunks, block_hash, prev_block_hash) = block?;

        let last_block_hash = self.data.lock().unwrap().last_block_hash;
        if let Some(block_hash) = last_block_hash {
            if block_hash != prev_block_hash {
                let mut data = self.data.lock().unwrap();
                data.last_block = data.last_handled_block;
                return None;
            }
        }

        print!("\rHeight: {height:?}");
        std::io::stdout().flush().expect("Flush failed");

        let indexed_data = client.get_chunk_indexed_data(chunks, height).await;
        self.set_indexed_data(
            height,
            indexed_data,
            client.unresolved_blocks.clone(),
            current_height,
            last_block,
            first_block,
            block_hash,
        );

        // Save data
        if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
            self.last_saved_time = Instant::now();
            let current_block_height = current_height;
            let data_file = self.data_file.clone();
            let data = self.data.lock().unwrap().clone();

            Some(tokio::spawn(async move {
                Self::save_data(&data, &data_file, current_block_height, first_block, height);
            }))
        } else {
            None
        }
    }
}
