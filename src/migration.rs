use crate::rpc::RPC;
use aurora_engine_migration_tool::StateData;
use borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::{AccountId, Balance, StorageUsage};
use std::collections::HashMap;
use std::path::PathBuf;

const MIGRATION_METHOD: &str = "migrate";
const MIGRATION_CHECK_METHOD: &str = "check_migration_correctness";
const RECORDS_COUNT_PER_TX: usize = 1000;

pub struct MigrationConfig {
    pub signer_account_id: String,
    pub signer_secret_key: String,
    pub contract: String,
}

pub struct Migration {
    pub rpc: RPC,
    pub data: StateData,
    pub config: MigrationConfig,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct MigrationInputData {
    pub accounts: HashMap<AccountId, Balance>,
    pub total_supply: Option<Balance>,
    pub account_storage_usage: Option<StorageUsage>,
    pub statistics_aurora_accounts_counter: Option<u64>,
    pub used_proofs: Vec<String>,
}

#[derive(Debug, BorshSerialize, BorshDeserialize, Eq, PartialEq)]
pub enum MigrationCheckResult {
    Success,
    AccountNotExist(AccountId),
    AccountAmount((AccountId, Balance)),
    TotalSupply(Balance),
    StorageUsage(StorageUsage),
    StatisticsCounter(u64),
    Proof(String),
}

impl Migration {
    pub async fn new(
        data_file: &PathBuf,
        signer_account_id: String,
        signer_secret_key: String,
    ) -> anyhow::Result<Self> {
        let data = std::fs::read(data_file).unwrap_or_default();
        let data: StateData = StateData::try_from_slice(&data[..]).expect("Failed parse data");

        Ok(Self {
            rpc: RPC::new().await?,
            data,
            config: MigrationConfig {
                signer_account_id: signer_account_id.clone(),
                signer_secret_key,
                contract: signer_account_id,
            },
        })
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        // Proofs migration
        let limit = RECORDS_COUNT_PER_TX;
        let mut i = 0;
        let mut proofs_count = 0;
        loop {
            let proofs = if i + limit >= self.data.proofs.len() {
                &self.data.proofs[i..]
            } else {
                &self.data.proofs[i..i + limit]
            };
            proofs_count += proofs.len();
            let migration_data = MigrationInputData {
                accounts: HashMap::new(),
                total_supply: None,
                account_storage_usage: None,
                statistics_aurora_accounts_counter: None,
                used_proofs: proofs.to_vec(),
            }
            .try_to_vec()
            .expect("Failed serialize");

            self.rpc
                .commit_tx(
                    self.config.signer_account_id.clone(),
                    self.config.signer_secret_key.clone(),
                    self.config.contract.clone(),
                    MIGRATION_METHOD.to_string(),
                    migration_data.clone(),
                )
                .await?;
            let res = self
                .rpc
                .request_view(
                    self.config.contract.clone(),
                    MIGRATION_CHECK_METHOD.to_string(),
                    migration_data,
                )
                .await?;
            let correctness = MigrationCheckResult::try_from_slice(&res[..]);

            println!("Proofs: {:?} [{:?}]", proofs_count, correctness);
            if i + limit >= self.data.proofs.len() {
                break;
            } else {
                i += limit;
            }
        }
        assert_eq!(proofs_count, self.data.proofs.len());

        let mut accounts: HashMap<AccountId, Balance> = HashMap::new();
        let mut accounts_count = 0;
        for (i, (account, amount)) in self.data.accounts.iter().enumerate() {
            let account = AccountId::try_from(account.to_string()).unwrap();
            accounts.insert(account.clone(), amount.as_u128());
            if accounts.len() < limit && i < self.data.accounts.len() - 1 {
                continue;
            }
            accounts_count += &accounts.len();

            let migration_data = MigrationInputData {
                accounts,
                total_supply: None,
                account_storage_usage: None,
                statistics_aurora_accounts_counter: None,
                used_proofs: vec![],
            }
            .try_to_vec()
            .expect("Failed serialize");

            self.rpc
                .commit_tx(
                    self.config.signer_account_id.clone(),
                    self.config.signer_secret_key.clone(),
                    self.config.contract.clone(),
                    MIGRATION_METHOD.to_string(),
                    migration_data.clone(),
                )
                .await?;
            let res = self
                .rpc
                .request_view(
                    self.config.contract.clone(),
                    MIGRATION_CHECK_METHOD.to_string(),
                    migration_data,
                )
                .await?;
            let correctness = MigrationCheckResult::try_from_slice(&res[..]);

            println!("Accounts: {:?} [{:?}]", accounts_count, correctness);
            // Clear
            accounts = HashMap::new();
        }
        assert_eq!(self.data.accounts.len(), accounts_count);

        let migration_data = MigrationInputData {
            accounts: HashMap::new(),
            total_supply: Some(3123),
            account_storage_usage: Some(22),
            statistics_aurora_accounts_counter: Some(334),
            used_proofs: vec![],
        }
        .try_to_vec()
        .expect("Failed serialize");

        self.rpc
            .commit_tx(
                self.config.signer_account_id.clone(),
                self.config.signer_secret_key.clone(),
                self.config.contract.clone(),
                MIGRATION_METHOD.to_string(),
                migration_data.clone(),
            )
            .await?;
        let res = self
            .rpc
            .request_view(
                self.config.contract.clone(),
                "check_migration_correctness\
                "
                .to_string(),
                migration_data,
            )
            .await?;

        let res = MigrationCheckResult::try_from_slice(&res[..]);
        println!("{:?}", res);

        Ok(())
    }
}
