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
    forward_block: Option<u64>,
    last_saved_time: Instant,
    last_forward_time: Instant,
}

impl Indexer {
    /// Init new indexer
    pub fn new<P: AsRef<Path>>(data_file: P, block_height: BlockHeight) -> anyhow::Result<Self> {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let mut data = IndexerData::try_from_slice(&data).unwrap_or_default();

        data.last_block = block_height - 1;
        if data.first_block > block_height {
            data.first_block = block_height;
        }

        Ok(Self {
            data: Arc::new(Mutex::new(data)),
            data_file: data_file.as_ref().to_path_buf(),
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
        indexed_data: IndexedData,
        missed_blocks: HashSet<BlockHeight>,
        current_block: BlockHeight,
        first_block: BlockHeight,
        last_block: BlockHeight,
        block_hash: CryptoHash,
    ) {
        let mut data = self.data.lock().unwrap();
        data.first_block = first_block;
        if data.first_block == 0 {
            data.first_block = last_block;
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
        let last_block = self.data.lock().unwrap().last_block + 1;
        let first_block = self.data.lock().unwrap().first_block;
        let mut current_height = if let Some(height) = self.forward_block {
            height
        } else {
            0
        };

        if self.forward_block.is_none() || self.last_forward_time.elapsed() > FORWARD_BLOCK_TIMEOUT
        {
            self.last_forward_time = Instant::now();
            if let Ok(block) = client.get_block(BlockKind::Latest).await {
                self.forward_block = Some(block.0);
                current_height = block.0
            }
        }

        let block = if last_block > current_height {
            println!(
                "Try to fetch block with height bigger than latest block. \
                          Sleep: {FORWARD_BLOCK_TIMEOUT:?}"
            );
            sleep(FORWARD_BLOCK_TIMEOUT).await;
            None
        } else if let Ok(block) = client.get_block(BlockKind::Height(last_block)).await {
            client.unresolved_blocks.remove(&last_block);
            Some(block)
        } else {
            // If block not found do not fail, just increment height
            let mut data = self.data.lock().unwrap();
            data.last_block = last_block;
            None
        };

        let (_, chunks, block_hash, prev_block_hash) = block?;

        let last_block_hash = self.data.lock().unwrap().last_block_hash;
        if let Some(block_hash) = last_block_hash {
            if block_hash != prev_block_hash {
                let mut data = self.data.lock().unwrap();
                data.last_block = data.last_handled_block;
                return None;
            }
        }

        print!("\rHeight: {last_block:?}");
        std::io::stdout().flush().expect("Flush failed");

        let indexed_data = client.get_chunk_indexed_data(chunks, last_block).await;
        self.set_indexed_data(
            indexed_data,
            client.unresolved_blocks.clone(),
            current_height,
            first_block,
            last_block,
            block_hash,
        );

        // Save data
        if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
            self.last_saved_time = Instant::now();
            let current_block_height = current_height;
            let data_file = self.data_file.clone();
            let data = self.data.lock().unwrap().clone();

            Some(tokio::spawn(async move {
                Self::save_data(
                    &data,
                    &data_file,
                    current_block_height,
                    first_block,
                    last_block,
                );
            }))
        } else {
            None
        }
    }
}
