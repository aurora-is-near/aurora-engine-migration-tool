//! # RPC
//! RPC toolset for effective communication with near-rpc for specific network.
//!
use near_crypto::Signer;
use near_jsonrpc_client::{methods, JsonRpcClient, MethodCallResult};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::hash::CryptoHash;
use near_primitives::transaction::{Action, FunctionCallAction, Transaction, TransactionV0};
use near_primitives::types::{BlockHeight, BlockReference};
use near_primitives::views::{ChunkHeaderView, FinalExecutionStatus};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::AccountId;
use std::collections::HashSet;
use std::time::Duration;

use self::error::CommitTx;

#[cfg(feature = "mainnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_MAINNET_RPC_URL;

#[cfg(feature = "mainnet-archival")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_MAINNET_ARCHIVAL_RPC_URL;

#[cfg(feature = "testnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_TESTNET_RPC_URL;

#[cfg(feature = "localnet")]
const NEAR_RPC_ADDRESS: &str = "http://127.0.0.1:3030";

/// NEAR-RPC has limits: 600 req/sec, so we need timeout per requests
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(90);

/// Gas for commit tx to blockchain (300 `TGas`)
const GAS_FOR_COMMIT_TX: u64 = 300_000_000_000_000;

/// Transactions receiver
pub const AURORA_CONTRACT: &str = "aurora";

/// How many retries per success request
const RETRIES_COUNT: u8 = 10;

/// Transaction action methods allowed for output parsing and
/// get `predecessor_account_id`
pub const ACTION_METHODS: &[&str] = &[
    "ft_transfer",
    "deposit",
    "ft_transfer_call",
    "withdraw",
    "finish_deposit",
    "storage_deposit",
    "storage_withdraw",
    "storage_unregister",
];

pub struct Client {
    /// NEAR-rpc client
    pub client: JsonRpcClient,
    /// One possible reason: https://stackoverflow.com/a/72230096
    pub unresolved_blocks: HashSet<BlockHeight>,
}

pub enum BlockKind {
    Latest,
    Height(BlockHeight),
}

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct ActionResultLog {
    pub accounts: Vec<AccountId>,
    pub method: String,
}

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct ActionResult {
    pub accounts: Vec<AccountId>,
    pub is_action_found: bool,
    pub log: Vec<ActionResultLog>,
}

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct IndexedResultLog {
    pub block_height: BlockHeight,
    pub actions: Vec<ActionResultLog>,
}

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct IndexedData {
    pub accounts: HashSet<AccountId>,
    pub logs: Vec<IndexedResultLog>,
}

impl Client {
    /// Init RPC with final (latest) flock height
    #[must_use]
    pub fn new() -> Self {
        Self {
            // Init ner-rpc client
            client: JsonRpcClient::connect(NEAR_RPC_ADDRESS),
            unresolved_blocks: HashSet::new(),
        }
    }

    /// Set missed blocks for RPC runner
    pub fn set_missed_blocks(&mut self, missed_blocks: HashSet<BlockHeight>) {
        self.unresolved_blocks = missed_blocks;
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
    ) -> anyhow::Result<(BlockHeight, Vec<ChunkHeaderView>, CryptoHash, CryptoHash)> {
        let block_reference = if let BlockKind::Height(height) = bloch_kind {
            BlockReference::BlockId(near_primitives::types::BlockId::Height(height))
        } else {
            BlockReference::Finality(near_primitives::types::Finality::Final)
        };
        let block = self
            .call(methods::block::RpcBlockRequest { block_reference })
            .await
            .inspect_err(|_e| {
                let mut msg = "Failed get block".to_string();
                if let BlockKind::Height(height) = bloch_kind {
                    self.unresolved_blocks.insert(height);
                    msg = format!("{msg}: {height:?}");
                }
                print_log(&msg);
            })?;

        Ok((
            block.header.height,
            block.chunks,
            block.header.hash,
            block.header.prev_hash,
        ))
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
        // Get signer key for Tx commit
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
                    public_key: signer.public_key().clone(),
                },
            })
            .await?;

        // Get access key nonce
        let current_nonce = match access_key_query_response.kind {
            QueryResponseKind::AccessKey(access_key) => access_key.nonce,
            _ => Err(CommitTx::AccessKey)?,
        };

        // Prepare transaction to commit
        let transaction = Transaction::V0(TransactionV0 {
            signer_id: signer.account_id.clone(),
            public_key: signer.public_key().clone(),
            nonce: current_nonce + 1,
            receiver_id: contract.parse()?,
            block_hash: access_key_query_response.block_hash,
            actions: vec![Action::FunctionCall(Box::from(FunctionCallAction {
                method_name: method,
                args,
                gas: GAS_FOR_COMMIT_TX,
                deposit: 0,
            }))],
        });

        println!(
            "nonce: {}, tx_hash: {:#?}",
            transaction.nonce(),
            transaction.get_hash_and_size().0
        );

        let request = methods::broadcast_tx_commit::RpcBroadcastTxCommitRequest {
            signed_transaction: transaction.sign(&Signer::InMemory(signer.clone())),
        };

        let mut retry = 0;
        // Trying commit tx with retry if failed
        loop {
            // Commit tx
            let mut res = self
                .client
                .call(&request)
                .await
                .map_err(|err| CommitTx::Commit(format!("{err:?}")));

            // Check response and set errors if it needs
            if let Ok(tx_res) = res {
                // If success - check response status
                match tx_res.status {
                    FinalExecutionStatus::SuccessValue(_) => return Ok(()),
                    FinalExecutionStatus::Failure(err) => {
                        res = Err(CommitTx::Status(format!("{err:?}")));
                    }
                    _ => res = Err(CommitTx::Status("Other".to_string())),
                }
            }

            // If request failed for some reason - retry request
            retry += 1;
            println!("\nRequest retry: {retry:?}");
            // If all retries failed it's incident, just panic
            assert!(
                retry <= RETRIES_COUNT,
                "Failed commit tx {RETRIES_COUNT:?} times: {res:?}",
            );
        }
    }

    /// Request view data for contract method.
    /// Return error if wrong response type or failViewed request
    pub async fn request_view(
        &self,
        contract: &str,
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
        // Response should contain only CallResult, if something other - return error
        if let QueryResponseKind::CallResult(result) = response.kind {
            Ok(result.result)
        } else {
            anyhow::bail!(CommitTx::View)
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
fn print_log(msg: &str) {
    #[cfg(feature = "log")]
    // Print with space shift
    println!(" {msg}");
}

mod error {
    #[derive(Debug)]
    pub enum CommitTx {
        AccessKey,
        Commit(String),
        View,
        Status(String),
    }

    impl std::error::Error for CommitTx {
        fn description(&self) -> &str {
            Box::leak(self.to_string().into_boxed_str())
        }
    }

    impl std::fmt::Display for CommitTx {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match self {
                Self::AccessKey => write!(f, "ERR_FAILED_GET_ACCESS_KEY"),
                Self::Commit(msg) => write!(f, "ERR_FAILED_COMMIT_TX: {msg}"),
                Self::View => write!(f, "ERR_FAILED_VIEW_TX"),
                Self::Status(msg) => write!(f, "ERR_TX_STATUS_FAIL: {msg}"),
            }
        }
    }
}
