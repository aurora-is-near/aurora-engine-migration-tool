use crate::rpc::{BlockKind, Client, IndexedData};
use futures::StreamExt;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use near_lake_framework::LakeConfigBuilder;
use near_primitives::hash::CryptoHash;
use near_primitives::types::BlockHeight;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

const SAVE_FILE_TIMEOUT: Duration = Duration::from_secs(60);
const FORWARD_BLOCK_TIMEOUT: Duration = Duration::from_secs(120);

// Information about indexed data that is saved to a file
// and will be loaded from the file when the program restarts.
#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct IndexerData {
    // Height of the first indexed block
    pub first_block: BlockHeight,
    // Height of the last block we attempted to index.
    // In the next iteration, we will index the block last_block + 1.
    pub last_block: BlockHeight,
    // Height of the last block we successfully indexed
    pub last_handled_block: BlockHeight,
    // The latest block in the NEAR network at the time of indexing.
    pub current_block: BlockHeight,
    // Hash of the last successfully processed block with the height last_handled_block.
    pub last_block_hash: Option<CryptoHash>,
    // A set of blocks that could not be successfully processed.
    pub missed_blocks: HashSet<BlockHeight>,
    // Indexed data: a list of accounts.
    pub data: IndexedData,
}

pub struct Indexer {
    // Data that is saved to a file every SAVE_FILE_TIMEOUT interval.
    pub data: Arc<Mutex<IndexerData>>,
    // The file in which the data is saved.
    pub data_file: PathBuf,
    // Height of the latest block in NEAR.
    forward_block: Option<u64>,
    // The time when the data was last saved to the file.
    last_saved_time: Instant,
    // The time when the height of the latest block in NEAR was last retrieved.
    last_forward_time: Instant,
}

impl Indexer {
    /// Init new indexer
    pub fn new<P: AsRef<Path>>(
        data_file: P,
        block_height: Option<BlockHeight>,
    ) -> anyhow::Result<Self> {
        // If file doesn't exist just return default data
        let data = std::fs::read(&data_file).unwrap_or_default();
        let mut data = IndexerData::try_from_slice(&data).unwrap_or_default();

        if let Some(block_height) = block_height {
            data.last_block = block_height - 1;
            if data.first_block > block_height {
                data.first_block = block_height;
            }
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
    }

    /// Save indexed data
    fn save_data<P: AsRef<Path>>(
        data: &IndexerData,
        data_file: P,
        current_block_height: BlockHeight,
        first_handled_block_height: BlockHeight,
        last_handled_block_height: BlockHeight,
    ) {
        std::fs::write(data_file, borsh::to_vec(&data).expect("Failed serialize"))
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
        let mut logs = indexed_data.logs;
        data.data.logs.append(&mut logs);
        data.missed_blocks = missed_blocks;
        data.last_block_hash = Some(block_hash);
    }

    /// Run indexing
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let last_block = self.data.lock().unwrap().last_block + 1;
        println!("Starting height: {last_block}");

        let lake_config = {
            let lake_builder = LakeConfigBuilder::default().start_block_height(last_block);
            if cfg!(feature = "mainnet") {
                lake_builder.mainnet().build()?
            } else if cfg!(feature = "testnet") {
                lake_builder.testnet().build()?
            } else {
                anyhow::bail!("Either 'mainnet' or 'testnet' feature must be enabled.");
            }
        };

        let (_, stream_receiver) = near_lake_framework::streamer(lake_config);
        let mut stream = tokio_stream::wrappers::ReceiverStream::new(stream_receiver);

        let mut client = Client::new();

        while let Some(streamer_message) = stream.next().await {
            self.handle_streamer_message(streamer_message, &mut client)
                .await;
        }
        Ok(())
    }

    async fn handle_streamer_message(
        &mut self,
        streamer_message: StreamerMessage,
        client: &mut Client,
    ) {
        let first_block = self.data.lock().unwrap().first_block;
        let last_block = self.data.lock().unwrap().last_block + 1;
        print!("\rHeight: {:?}", last_block);
        std::io::stdout().flush().expect("Flush failed");

        let chunks = streamer_message.block.chunks;
        let indexed_data = client.get_chunk_indexed_data(chunks, last_block).await;
        self.set_indexed_data(
            indexed_data,
            client.unresolved_blocks.clone(),
            streamer_message.block.header.height,
            first_block,
            last_block,
            streamer_message.block.header.hash,
        );

        if self.last_saved_time.elapsed() > SAVE_FILE_TIMEOUT {
            self.last_saved_time = Instant::now();
            let data_file = self.data_file.clone();
            let data = self.data.lock().unwrap().clone();

            Self::save_data(
                &data,
                &data_file,
                streamer_message.block.header.height,
                first_block,
                last_block,
            );
        }
    }
}
