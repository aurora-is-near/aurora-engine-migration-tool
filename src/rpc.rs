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

/// Transactions receiver
const AURORA_CONTRACT: &str = "aurora";

/// How many retries per success request
const RETRIES_COUNT: u8 = 10;

/// Transaction action method, allowed for output parsing
const ACTION_METODS: &[&str] = &[
    "ft_transfer",
    "ft_transfer_call",
    "withdraw",
    "finish_deposit",
];

#[derive(Debug)]
pub enum CommitTxError {
    AccessKeyFail,
    CommitFail(String),
    ViewFail,
    StatusFail(String),
    // NotStarted,
    // Started,
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
            Self::CommitFail(msg) => write!(f, "ERR_FAILED_COMMIT_TX: {}", msg),
            Self::ViewFail => write!(f, "ERR_FAILED_VIEW_TX"),
            Self::StatusFail(msg) => write!(f, "ERR_TX_STATUS_FAIL: {}", msg),
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
    /// Init RPC with final (latest) flock height
    pub async fn new() -> anyhow::Result<Self> {
        // Init ner-rpc client
        let client = JsonRpcClient::connect(NEAR_RPC_ADDRESS);

        // Get final (latest) block
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

    /// Wrap rpc-client calls.
    /// All calls should have timeout, it's related to
    /// restrictions of request count per minute: 600 per/min
    pub async fn call<M>(&self, method: M) -> MethodCallResult<M::Response, M::Error>
    where
        M: methods::RpcMethod,
    {
        tokio::time::sleep(REQUEST_TIMEOUT).await;
        self.client.call(method).await
    }

    /// Get block data with Block kind request
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

    /// Get action output for chunk transaction (including receipt output)
    pub async fn get_actions_output(&mut self, tx: &SignedTransactionView) -> Vec<String> {
        let mut results: Vec<String> = vec![];
        for action in &tx.actions {
            // Check action method and filter it
            let (method_name, _args) = match action {
                ActionView::FunctionCall {
                    method_name, args, ..
                } => (method_name, args),
                _ => continue,
            };
            if !ACTION_METODS.contains(&method_name.as_str()) {
                continue;
            }
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
        // Fetch all chunks from block
        for chunk in chunks {
            // Get chunk data
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
            // Fetch chunk transactions
            for tx in &chunk_data.transactions {
                // We should process only specific receiver
                if tx.receiver_id.as_str() != AURORA_CONTRACT {
                    continue;
                }
                // Get actions list from transaction
                let mut outputs = self.get_actions_output(tx).await;
                results.append(&mut outputs);
            }
        }
        results
    }

    /// Commit transaction and wait respond. It should retry if it's fail
    /// for some reason.
    /// Return error if request call failed, or status type not Success
    /// after retries.
    pub async fn commit_tx(
        &self,
        signer_account_id: String,
        signer_secret_key: String,
        contract: String,
        method: String,
        args: Vec<u8>,
    ) -> anyhow::Result<()> {
        // Get signer key for commiting tx
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

        // Get access key nonce
        let current_nonce = match access_key_query_response.kind {
            QueryResponseKind::AccessKey(access_key) => access_key.nonce,
            _ => Err(CommitTxError::AccessKeyFail)?,
        };

        // Prepare transaction to commit
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

        let mut retry = 0;
        // Trying commit tx with retry if failed
        loop {
            // Commit tx
            let mut res = self
                .client
                .call(&request)
                .await
                .map_err(|err| CommitTxError::CommitFail(format!("{:?}", err)));
            // Check response and set errors if it needs
            if let Ok(tx_res) = res {
                // If success - check response status
                match tx_res.status {
                    FinalExecutionStatus::SuccessValue(_) => return Ok(()),
                    FinalExecutionStatus::Failure(err) => {
                        res = Err(CommitTxError::StatusFail(format!("{:?}", err)))
                    }
                    _ => res = Err(CommitTxError::StatusFail("Other".to_string())),
                }
            }

            // If request failed for some reason - retry request
            retry += 1;
            println!("\nRequst retry: {:?}", retry);
            // If all retries failed it's incident, just panic
            if retry > RETRIES_COUNT {
                panic!("Failed commit tx {:?} times: {:?}", RETRIES_COUNT, res);
            }
        }
    }

    /// Request view data for contract method.
    /// Return error if wrong response type or failed request
    pub async fn request_view(
        &self,
        contract: String,
        method: String,
        args: Vec<u8>,
    ) -> anyhow::Result<Vec<u8>> {
        // Request fro final (latest) block
        let request = methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(near_primitives::types::Finality::Final),
            request: near_primitives::views::QueryRequest::CallFunction {
                account_id: contract.parse()?,
                method_name: method,
                args: near_primitives::types::FunctionArgs::from(args),
            },
        };

        let response = self.client.call(request).await?;
        // Response should containt onlt CallResult, if something other - return error
        if let QueryResponseKind::CallResult(result) = response.kind {
            return Ok(result.result);
        }
        Err(CommitTxError::ViewFail)?
    }
}
