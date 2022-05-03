use crate::rpc::RPC;
use aurora_engine_migration_tool::StateData;
use borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::{AccountId, Balance, StorageUsage};
use std::collections::HashMap;
use std::path::PathBuf;

const MIGRATION_METHOD: &str = "migrate";
const MIGRATION_CHECK_METHOD: &str = "check_migration_correctness";
const RECORDS_COUNT_PER_TX: usize = 750;

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
    AccountNotExist(Vec<AccountId>),
    AccountAmount(HashMap<AccountId, Balance>),
    TotalSupply(Balance),
    StorageUsage(StorageUsage),
    StatisticsCounter(u64),
    Proof(Vec<String>),
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

    /// Commit migration data as transaction call
    async fn commit_migration(
        &self,
        migration_data: Vec<u8>,
        msg: &str,
        counter: usize,
    ) -> anyhow::Result<()> {
        self.rpc
            .commit_tx(
                self.config.signer_account_id.clone(),
                self.config.signer_secret_key.clone(),
                self.config.contract.clone(),
                MIGRATION_METHOD.to_string(),
                migration_data.clone(),
            )
            .await?;
        print!("\r{msg}: {counter}");
        Ok(())
    }

    /// Send request to check migration correctness
    async fn check_migration(
        &self,
        msg: &str,
        migration_data: Vec<u8>,
        counter: usize,
    ) -> anyhow::Result<()> {
        let res = self
            .rpc
            .request_view(
                self.config.contract.clone(),
                MIGRATION_CHECK_METHOD.to_string(),
                migration_data,
            )
            .await?;
        let correctness = MigrationCheckResult::try_from_slice(&res[..]).unwrap();
        match correctness {
            MigrationCheckResult::Proof(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len())
            }
            MigrationCheckResult::AccountNotExist(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len())
            }
            MigrationCheckResult::AccountAmount(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len())
            }
            MigrationCheckResult::Success => print!("\r{msg}: {counter} [{:?}]", correctness),
            _ => {
                if let MigrationCheckResult::TotalSupply(_) = correctness {
                    println!("{msg} [Missed field: {:?}]", correctness)
                }
                if let MigrationCheckResult::StorageUsage(_) = correctness {
                    println!("{msg} [Missed field: {:?}]", correctness)
                }
                if let MigrationCheckResult::StatisticsCounter(_) = correctness {
                    println!("{msg} [Missed field: {:?}]", correctness)
                }
            }
        }
        Ok(())
    }

    /// Run migration process
    pub async fn run(&self) -> anyhow::Result<()> {
        // Data limit per transaction
        let limit = RECORDS_COUNT_PER_TX;

        // Proofs migration
        let mut i = 0;
        let mut proofs_count = 0;
        let mut reproducible_data_for_proofs: Vec<(Vec<u8>, usize)> = vec![];
        loop {
            let proofs = if i + limit >= self.data.proofs.len() {
                &self.data.proofs[i..]
            } else {
                &self.data.proofs[i..i + limit]
            }
            .to_vec();

            proofs_count += proofs.len();
            let migration_data = MigrationInputData {
                accounts: HashMap::new(),
                total_supply: None,
                account_storage_usage: None,
                statistics_aurora_accounts_counter: None,
                used_proofs: proofs,
            }
            .try_to_vec()
            .expect("Failed serialize");
            reproducible_data_for_proofs.push((migration_data.clone(), proofs_count));

            self.commit_migration(migration_data, "Proofs", proofs_count)
                .await?;

            if i + limit >= self.data.proofs.len() {
                break;
            } else {
                i += limit;
            }
        }
        assert_eq!(proofs_count, self.data.proofs.len());

        // Accounts migration
        println!();
        let mut accounts: HashMap<AccountId, Balance> = HashMap::new();
        let mut accounts_count = 0;
        let mut reproducible_data_for_accounts: Vec<(Vec<u8>, usize)> = vec![];
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
            reproducible_data_for_accounts.push((migration_data.clone(), accounts_count));

            self.commit_migration(migration_data, "Accounts", accounts_count)
                .await?;

            // Clear
            accounts = HashMap::new();
        }
        assert_eq!(self.data.accounts.len(), accounts_count);

        // Migrate Contract data
        println!();
        let contract_migration_data = MigrationInputData {
            accounts: HashMap::new(),
            total_supply: Some(self.data.contract_data.total_eth_supply_on_near.as_u128()),
            account_storage_usage: Some(self.data.contract_data.account_storage_usage),
            statistics_aurora_accounts_counter: Some(self.data.accounts_counter),
            used_proofs: vec![],
        }
        .try_to_vec()
        .expect("Failed serialize");

        self.commit_migration(contract_migration_data.clone(), "Contract data", 1)
            .await?;

        //=====================================
        // Checking the correctness and integrity of data, regardless of
        // the migration process

        println!();
        for (migration_data, counter) in reproducible_data_for_proofs {
            self.check_migration("Proofs", migration_data, counter)
                .await?;
        }

        println!();
        for (migration_data, counter) in reproducible_data_for_accounts {
            self.check_migration("Accounts:", migration_data, counter)
                .await?;
        }

        println!();
        self.check_migration("Contract data:", contract_migration_data, 0)
            .await?;

        Ok(())
    }
}
