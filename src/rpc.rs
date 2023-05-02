//! # RPC
//! RPC toolset for effective communication with near-rpc for specific network.
//!
use near_jsonrpc_client::{methods, JsonRpcClient, MethodCallResult};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::transaction::{Action, FunctionCallAction, Transaction};
use near_primitives::types::{BlockHeight, BlockReference};
use near_primitives::views::{ActionView, ChunkHeaderView, FinalExecutionStatus};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::AccountId;
use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

use self::error::CommitTx;

#[cfg(feature = "mainnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_MAINNET_RPC_URL;

#[cfg(feature = "mainnet-archival")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_MAINNET_ARCHIVAL_RPC_URL;

#[cfg(feature = "testnet")]
const NEAR_RPC_ADDRESS: &str = near_jsonrpc_client::NEAR_TESTNET_RPC_URL;

/// NEAR-RPC has limits: 600 req/sec, so we need timeout per requests
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(90);

/// Gas for commit tx to blockchain (300 `TGas`)
const GAS_FOR_COMMIT_TX: u64 = 300_000_000_000_000;

/// Transactions receiver
pub const AURORA_CONTRACT: &str = "aurora";

/// How many retries per success request
const RETRIES_COUNT: u8 = 10;

/// Transaction action method, allowed for output parsing
const ACTION_METHODS: &[&str] = &[
    "ft_transfer",
    "deposit",
    "ft_transfer_call",
    "withdraw",
    "finish_deposit",
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
    pub proof: String,
    pub method: String,
}

#[derive(Debug, Default, Clone, BorshSerialize, BorshDeserialize)]
pub struct ActionResult {
    pub accounts: Vec<AccountId>,
    pub proofs: Vec<String>,
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
    pub proofs: HashSet<String>,
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
    ) -> anyhow::Result<(BlockHeight, Vec<ChunkHeaderView>)> {
        let block_reference = if let BlockKind::Height(height) = bloch_kind {
            BlockReference::BlockId(near_primitives::types::BlockId::Height(height))
        } else {
            BlockReference::Finality(near_primitives::types::Finality::Final)
        };
        let block = self
            .call(methods::block::RpcBlockRequest { block_reference })
            .await
            .map_err(|e| {
                let mut msg = "Failed get block".to_string();
                if let BlockKind::Height(height) = bloch_kind {
                    self.unresolved_blocks.insert(height);
                    msg = format!("{}: {:?}", msg, height);
                }
                print_log(&msg);
                e
            })?;

        Ok((block.header.height, block.chunks))
    }

    /// Get action output for chunk transaction (including receipt output)
    /// It includes: Accounts, Proof keys
    pub fn get_actions_data(&mut self, actions: Vec<ActionView>) -> ActionResult {
        let mut result = ActionResult::default();

        for action in actions {
            // Check action method and filter it
            if let ActionView::FunctionCall {
                method_name, args, ..
            } = action
            {
                if ACTION_METHODS.contains(&method_name.as_str()) {
                    let (accounts, proof) = self.parse_action_argument(&method_name, &args);

                    result.is_action_found = true;
                    result.log.push(ActionResultLog {
                        accounts: accounts.clone(),
                        proof: proof.clone().unwrap_or_default(),
                        method: method_name,
                    });
                    result.accounts.extend(accounts);

                    if let Some(proof) = proof {
                        result.proofs.push(proof);
                    }
                }
            }
        }

        result
    }

    /// Parse action arguments and return accounts and proof keys
    #[must_use]
    pub fn parse_action_argument(
        &self,
        method: &str,
        args: &[u8],
    ) -> (Vec<AccountId>, Option<String>) {
        use serde::Deserialize;

        match method {
            "ft_transfer" => {
                #[derive(Debug, Deserialize)]
                pub struct FtTransferArgs {
                    pub receiver_id: AccountId,
                    pub amount: U128,
                    pub memo: Option<String>,
                }
                if let Ok(res) = serde_json::from_slice::<FtTransferArgs>(args) {
                    print_log("ft_transfer");
                    (vec![res.receiver_id], None)
                } else {
                    print_log(" Failed deserialize FtTransferArgs");
                    (vec![], None)
                }
            }
            "ft_transfer_call" => {
                #[derive(Debug, Deserialize)]
                pub struct FtTransferCallArgs {
                    pub receiver_id: AccountId,
                    pub amount: U128,
                    pub memo: Option<String>,
                    pub msg: String,
                }
                if let Ok(res) = serde_json::from_slice::<FtTransferCallArgs>(args) {
                    print_log("ft_transfer_call");
                    (vec![res.receiver_id], None)
                } else {
                    print_log("Failed deserialize FtTransferCallArgs");
                    (vec![], None)
                }
            }
            "withdraw" => {
                print_log(" Withdraw");
                (vec![], None)
            }
            "finish_deposit" => {
                #[derive(Debug, Clone, BorshDeserialize)]
                pub struct FinishDepositArgs {
                    pub new_owner_id: AccountId,
                    pub amount: u128,
                    pub proof_key: String,
                    pub relayer_id: AccountId,
                    pub fee: u128,
                    pub msg: Option<Vec<u8>>,
                }
                if let Ok(res) = FinishDepositArgs::try_from_slice(args) {
                    print_log("finish_deposit");
                    (vec![res.new_owner_id, res.relayer_id], Some(res.proof_key))
                } else {
                    print_log("Failed deserialize FinishDepositArgs");
                    (vec![], None)
                }
            }
            "deposit" => {
                print_log("deposit");
                (vec![], None)
            }
            _ => (vec![], None),
        }
    }

    /// Get transactions and receipts indexed data from chunks.
    /// Return indexed data including actions log.
    pub async fn get_chunk_indexed_data(
        &mut self,
        chunks: Vec<ChunkHeaderView>,
        block_height: BlockHeight,
    ) -> IndexedData {
        let mut results = IndexedData {
            accounts: HashSet::new(),
            proofs: HashSet::new(),
            logs: vec![],
        };

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
                print_log("Failed get chunk");
                // Set block as unresolved
                self.unresolved_blocks.insert(block_height);
                continue;
            };

            // Fetch chunk transactions
            for tx in &chunk_data.transactions {
                // We should process only specific receiver
                if tx.receiver_id.as_str() != AURORA_CONTRACT {
                    continue;
                }
                // Get actions and proof keys from transaction
                let res = self.get_actions_data(tx.actions.clone());
                // Added predecessor account
                if res.is_action_found {
                    results
                        .accounts
                        .insert(AccountId::from_str(tx.signer_id.as_str()).unwrap());
                    results.accounts.insert(AURORA_CONTRACT.parse().unwrap());

                    let mut log = res.log;
                    if !log.is_empty() {
                        log[0]
                            .accounts
                            .push(AccountId::from_str(tx.signer_id.as_str()).unwrap());
                        log[0].accounts.push(AURORA_CONTRACT.parse().unwrap());
                    }
                    results.logs.push(IndexedResultLog {
                        block_height,
                        actions: log,
                    });
                }
                for account in res.accounts {
                    results.accounts.insert(account);
                }
                for proof in res.proofs {
                    results.proofs.insert(proof);
                }
            }

            // Fetch chunk transactions for receipts
            for receipt in &chunk_data.receipts {
                // We should process only specific receiver
                if receipt.receiver_id.as_str() != AURORA_CONTRACT {
                    continue;
                }

                // Get actions accounts from receipt
                if let near_primitives::views::ReceiptEnumView::Action {
                    signer_id, actions, ..
                } = receipt.receipt.clone()
                {
                    let res = self.get_actions_data(actions);
                    // Added predecessor account
                    if res.is_action_found {
                        results
                            .accounts
                            .insert(AccountId::from_str(signer_id.as_str()).unwrap());
                        results
                            .accounts
                            .insert(AccountId::from_str(receipt.predecessor_id.as_str()).unwrap());
                        results
                            .accounts
                            .insert(AccountId::from_str(receipt.receiver_id.as_str()).unwrap());

                        let mut log = res.log;
                        if !log.is_empty() {
                            log[0]
                                .accounts
                                .push(AccountId::from_str(signer_id.as_str()).unwrap());
                            log[0].accounts.push(
                                AccountId::from_str(receipt.predecessor_id.as_str()).unwrap(),
                            );
                            log[0]
                                .accounts
                                .push(AccountId::from_str(receipt.receiver_id.as_str()).unwrap());
                        }
                        results.logs.push(IndexedResultLog {
                            block_height,
                            actions: log,
                        });
                    }
                    for account in res.accounts {
                        results.accounts.insert(account);
                    }
                    for proof in res.proofs {
                        results.proofs.insert(proof);
                    }
                }
            }
        }
        // Flow passed successfully - remove block
        if self.unresolved_blocks.contains(&block_height) {
            self.unresolved_blocks.remove(&block_height);
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
                    public_key: signer.public_key.clone(),
                },
            })
            .await?;

        // Get access key nonce
        let current_nonce = match access_key_query_response.kind {
            QueryResponseKind::AccessKey(access_key) => access_key.nonce,
            _ => Err(CommitTx::AccessKey)?,
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
                .map_err(|err| CommitTx::Commit(format!("{:?}", err)));
            // Check response and set errors if it needs
            if let Ok(tx_res) = res {
                // If success - check response status
                match tx_res.status {
                    FinalExecutionStatus::SuccessValue(_) => return Ok(()),
                    FinalExecutionStatus::Failure(err) => {
                        res = Err(CommitTx::Status(format!("{:?}", err)));
                    }
                    _ => res = Err(CommitTx::Status("Other".to_string())),
                }
            }

            // If request failed for some reason - retry request
            retry += 1;
            println!("\nRequest retry: {:?}", retry);
            // If all retries failed it's incident, just panic
            assert!(
                retry <= RETRIES_COUNT,
                "Failed commit tx {:?} times: {:?}",
                RETRIES_COUNT,
                res
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
                Self::Commit(msg) => write!(f, "ERR_FAILED_COMMIT_TX: {}", msg),
                Self::View => write!(f, "ERR_FAILED_VIEW_TX"),
                Self::Status(msg) => write!(f, "ERR_TX_STATUS_FAIL: {}", msg),
            }
        }
    }
}
