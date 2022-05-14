//! # RPC
//! RPC toolset for effective communication with near-rpc for specific network.
use near_jsonrpc_client::{methods, JsonRpcClient, MethodCallResult};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::hash::CryptoHash;
use near_primitives::transaction::{Action, FunctionCallAction, Transaction};
use near_primitives::types::{BlockHeight, BlockReference};
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

/// Gas for commit tx to blockchain (300 TGas)
const GAS_FOR_COMMIT_TX: u64 = 300_000_000_000_000;

const AURORA_CONTRACT: &str = "aurora";

#[derive(Debug)]
pub enum CommitTxError {
    AccessKeyFail,
    CommitFail,
    ViewFail,
    StatusFail,
    StatusFailMsg(String),
}

impl std::error::Error for CommitTxError {
    fn description(&self) -> &str {
        Box::leak(self.to_string().into_boxed_str())
    }
}

impl std::fmt::Display for CommitTxError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::AccessKeyFail => write!(f, "ERR_FAILED_GET_ACCESS_KEY"),
            Self::CommitFail => write!(f, "ERR_FAILED_COMMIT_TX"),
            Self::ViewFail => write!(f, "ERR_FAILED_VIEW_TX"),
            Self::StatusFailMsg(msg) => write!(f, "ERR_TX_STATUS_FAIL: {}", msg),
            Self::StatusFail => write!(f, "ERR_TX_STATUS_FAIL"),
        }
    }
}
pub type TransactionView = (near_primitives::types::AccountId, CryptoHash);

pub struct RPC {
    pub client: JsonRpcClient,
    pub latest_block_height: BlockHeight,
    pub unresolved_blocks: HashSet<BlockHeight>,
    pub unresolved_chunks: HashSet<CryptoHash>,
    pub unresolved_txs: HashSet<CryptoHash>,
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
            unresolved_chunks: HashSet::new(),
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
    pub async fn get_actions_output(&mut self, tx: &SignedTransactionView) -> Vec<String> {
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
                self.unresolved_txs.insert(tx.hash);
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

    /// Get transactions outcome from chunks
    pub async fn get_transactions_outcome(&mut self, chunks: Vec<ChunkHeaderView>) -> Vec<String> {
        let mut results = vec![];
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
                self.unresolved_chunks.insert(chunk.chunk_hash);
                continue;
            };
            for tx in &chunk_data.transactions {
                if tx.receiver_id.as_str() != AURORA_CONTRACT {
                    continue;
                }
                let mut outputs = self.get_actions_output(tx).await;
                results.append(&mut outputs);
            }
        }
        results
    }

    /// Commit transaction and wait respond
    pub async fn commit_tx(
        &self,
        signer_account_id: String,
        signer_secret_key: String,
        contract: String,
        method: String,
        args: Vec<u8>,
    ) -> anyhow::Result<()> {
        // tokio::time::sleep(SLEEP_BETWEEN_TX).await;
        let signer = near_crypto::InMemorySigner::from_secret_key(
            signer_account_id.parse()?,
            signer_secret_key.parse()?,
        );

        let access_key_query_response = self
            .client
            .call(methods::query::RpcQueryRequest {
                block_reference: BlockReference::latest(),
                request: near_primitives::views::QueryRequest::ViewAccessKey {
                    account_id: signer.account_id.clone(),
                    public_key: signer.public_key.clone(),
                },
            })
            .await?;

        let current_nonce = match access_key_query_response.kind {
            QueryResponseKind::AccessKey(access_key) => access_key.nonce,
            _ => Err(CommitTxError::AccessKeyFail)?,
        };

        let transaction = Transaction {
            signer_id: signer.account_id.clone(),
            public_key: signer.public_key.clone(),
            nonce: current_nonce + 1,
            receiver_id: contract.parse()?,
            block_hash: access_key_query_response.block_hash,
            actions: vec![Action::FunctionCall(FunctionCallAction {
                method_name: method,
                args,
                gas: GAS_FOR_COMMIT_TX,
                deposit: 0,
            })],
        };

        let request = methods::broadcast_tx_commit::RpcBroadcastTxCommitRequest {
            signed_transaction: transaction.sign(&signer),
        };

        let res = self
            .client
            .call(request)
            .await
            .map_err(|err| CommitTxError::CommitFail)?;

        match res.status {
            FinalExecutionStatus::SuccessValue(_) => Ok(()),
            FinalExecutionStatus::Failure(_) => {
                Err(CommitTxError::StatusFailMsg(format!("{:?}", msg)))?
            }
            _ => Err(CommitTxError::StatusFail)?,
        }
    }

    /// Request view data for contract method
    pub async fn request_view(
        &self,
        contract: String,
        method: String,
        args: Vec<u8>,
    ) -> anyhow::Result<Vec<u8>> {
        let request = methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(near_primitives::types::Finality::Final),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: contract.parse()?,
                method_name: method,
                args: near_primitives::types::FunctionArgs::from(args),
            },
        };

        let response = self.client.call(request).await?;
        if let QueryResponseKind::CallResult(result) = response.kind {
            return Ok(result.result);
        }
        Err(CommitTxError::ViewFail)?
    }
}
