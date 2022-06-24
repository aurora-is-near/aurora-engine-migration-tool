//! # RPC
//! RPC toolset for effective communication with near-rpc for specific network.
use near_jsonrpc_client::{methods, JsonRpcClient, MethodCallResult};
use near_primitives::hash::CryptoHash;
use near_primitives::types::BlockHeight;
use near_primitives::views::{
    ActionView, ChunkHeaderView, FinalExecutionStatus, SignedTransactionView,
};
use std::collections::HashSet;
use std::time::Duration;

#[cfg(feature = "mainnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_MAINNET_RPC_URL;

#[cfg(feature = "testnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_TESTNET_RPC_URL;

/// NEAR-RPC has limits: 600 req/sec, so we need timeout per requests
const REQUEST_TIMEOUT: Duration = Duration::from_millis(50);

const AURORA_CONTRACT: &str = "aurora";

pub type TransactionView = (near_primitives::types::AccountId, CryptoHash);

pub struct RPC {
    pub client: JsonRpcClient,
    pub latest_block_height: BlockHeight,
    pub unresolved_blocks: HashSet<BlockHeight>,
    pub unresolved_txs: HashSet<BlockHeight>,
}

pub enum BlockKind {
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
    ) -> anyhow::Result<(BlockHeight, Vec<ChunkHeaderView>)> {
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
            .call(methods::block::RpcBlockRequest { block_reference })
            .await
            .map_err(|e| {
                println!("Failed get block");
                if let BlockKind::Height(height) = bloch_kind {
                    self.unresolved_blocks.insert(height);
                }
                e
            })?;
        Ok((block.header.height, block.chunks))
    }

    /// Get action output for chunk transactions (including receipt output)
    pub async fn get_actions_output(&self, tx: &SignedTransactionView) -> Vec<String> {
        let mut results: Vec<String> = vec![];
        for action in &tx.actions {
            let method_name = match action {
                ActionView::FunctionCall { method_name, .. } => method_name,
                _ => continue,
            };
            println!("\n\n{:?} ", method_name);
            let outcome = if let Ok(tx_info) = self
                .call(methods::tx::RpcTransactionStatusRequest {
                    transaction_info: methods::tx::TransactionInfo::TransactionId {
                        hash: tx.hash,
                        account_id: tx.signer_id.clone(),
                    },
                })
                .await
            {
                println!("Tx: {:#?}\n", tx_info.status);
                match tx_info.status {
                    FinalExecutionStatus::SuccessValue(_) => {
                        let mut data = vec![tx_info.transaction_outcome];
                        let mut receipts_outcome = tx_info.receipts_outcome;
                        data.append(&mut receipts_outcome);
                        data
                    }
                    _ => continue,
                }
            } else {
                println!("Failed get tx: {:?}", tx.hash);
                continue;
            };
            let mut outputs: Vec<String> = outcome.iter().fold(vec![], |mut res, o| {
                let mut log = o.outcome.logs.clone();
                res.append(&mut log);
                res
            });
            results.append(&mut outputs);
        }
        results
    }

    /// Get transactions from chunks
    pub async fn get_transactions(
        &mut self,
        chunks: Vec<ChunkHeaderView>,
    ) -> anyhow::Result<Vec<TransactionView>> {
        for chunk in chunks {
            let chunk_data = if let Ok(chunk_data) = self
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
                println!("Failed get chunk: {:?}", chunk.chunk_hash);
                continue;
            };
            for tx in &chunk_data.transactions {
                if tx.receiver_id.as_str() != AURORA_CONTRACT {
                    continue;
                }
                let _outputs = self.get_actions_output(tx).await;
            }
        }
        Ok(vec![])
    }
}
