//! # RPC
//! RPC toolset for effective communication with near-rpc for specific network.
use near_jsonrpc_client::{methods, JsonRpcClient, MethodCallResult};
use near_primitives::hash::CryptoHash;
use near_primitives::types::BlockHeight;
use near_primitives::views::ChunkHeaderView;
use std::collections::HashSet;
use std::time::Duration;

#[cfg(feature = "mainnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_MAINNET_RPC_URL;

#[cfg(feature = "testnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_TESTNET_RPC_URL;

/// NEAR-RPC has limits: 600 req/sec, so we need timeout per requests
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1000);
/// Dedicated NEAR shard for Aurora contract
const AURORA_CONTRACT_SHARD: u8 = 3;

pub type TransactionView = (near_primitives::types::AccountId, CryptoHash);

pub struct RPC {
    pub client: JsonRpcClient,
    pub latest_block_height: BlockHeight,
    pub unresolved_blocks: HashSet<BlockHeight>,
    pub unresolved_txs: HashSet<BlockHeight>,
}

enum BlockKind {
    Latest,
    Height(BlockHeight),
}

impl RPC {
    /// Init rpc-client with final (latest) flock height
    pub async fn new() -> anyhow::Result<Self> {
        let client = JsonRpcClient::connect(NEAR_RPC_ADDRESS);

        let block_reference = near_primitives::types::BlockReference::Finality(
            near_primitives::types::Finality::Final,
        );
        let block = client
            .call(methods::block::RpcBlockRequest {
                block_reference: block_reference.clone(),
            })
            .await
            .expect("Failed get latest block");

        Ok(Self {
            client,
            latest_block_height: block.header.height,
            unresolved_blocks: HashSet::new(),
            unresolved_txs: HashSet::new(),
        })
    }

    /// Wrap rpc-client calls
    pub async fn call<M>(&self, method: M) -> MethodCallResult<M::Response, M::Error>
    where
        M: methods::RpcMethod,
    {
        tokio::time::sleep(REQUEST_TIMEOUT).await;
        self.client.call(method).await
    }

    pub async fn get_block(
        &mut self,
        bloch_kind: BlockKind,
    ) -> (BlockHeight, Vec<ChunkHeaderView>) {
        let block_reference = if let BlockKind::Height(height) = bloch_kind {
            near_primitives::types::BlockReference::BlockId(
                near_primitives::types::BlockId::Height(height),
            )
        } else {
            near_primitives::types::BlockReference::Finality(
                near_primitives::types::Finality::Final,
            )
        };
        let block = self
            .client
            .call(methods::block::RpcBlockRequest { block_reference })
            .await
            .expect("Failed get latest block");
        (block.header.height, block.chunks)
    }

    /// Get transactions from chunks
    pub async fn get_transactions(
        &mut self,
        chunks: Vec<ChunkHeaderView>,
    ) -> anyhow::Result<Vec<TransactionView>> {
        for chunk in chunks {
            let chunk_data = if let Ok(chunk_data) = self
                .client
                .call(methods::chunk::RpcChunkRequest {
                    chunk_reference:
                        near_jsonrpc_primitives::types::chunks::ChunkReference::ChunkHash {
                            chunk_id: chunk.chunk_hash,
                        },
                })
                .await
            {
                chunk_data
            } else {
                continue;
            };
        }
        Ok(vec![])
    }
}
