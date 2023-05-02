use crate::rpc::{Client, REQUEST_TIMEOUT};
use aurora_engine_migration_tool::{FungibleToken, StateData};
use aurora_engine_types::{account_id::AccountId, types::NEP141Wei};
use borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::{Balance, StorageUsage};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

const MIGRATION_METHOD: &str = "migrate";
const MIGRATION_CHECK_METHOD: &str = "check_migration_correctness";
const RECORDS_COUNT_PER_TX: usize = 750;

pub struct MigrationConfig {
    pub signer_account_id: String,
    pub signer_secret_key: String,
    pub contract: String,
}

pub struct Migration {
    pub client: Client,
    pub data: StateData,
    pub config: MigrationConfig,
}

#[derive(Debug, BorshDeserialize, BorshSerialize)]
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
    pub fn new<P: AsRef<Path>>(
        data_file: P,
        signer_account_id: String,
        signer_secret_key: String,
    ) -> anyhow::Result<Self> {
        let data = std::fs::read(data_file).unwrap_or_default();
        let data: StateData = StateData::try_from_slice(&data)?;

        Ok(Self {
            client: Client::new(),
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
        self.client
            .commit_tx(
                self.config.signer_account_id.clone(),
                self.config.signer_secret_key.clone(),
                self.config.contract.clone(),
                MIGRATION_METHOD.to_string(),
                migration_data,
            )
            .await?;
        print!("\r{msg}: {counter}");
        std::io::stdout().flush()?;
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
            .client
            .request_view(
                &self.config.contract,
                MIGRATION_CHECK_METHOD.to_string(),
                migration_data,
            )
            .await?;
        let correctness = MigrationCheckResult::try_from_slice(&res).unwrap();
        match correctness {
            MigrationCheckResult::Proof(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len());
            }
            MigrationCheckResult::AccountNotExist(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len());
            }
            MigrationCheckResult::AccountAmount(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len());
            }
            MigrationCheckResult::Success => {
                print!("\r{msg}: {counter} [{:?}]", correctness);
                std::io::stdout().flush()?;
            }
            _ => {
                if let MigrationCheckResult::TotalSupply(_) = correctness {
                    println!("{msg} [Missed field: {:?}]", correctness);
                }
                if let MigrationCheckResult::StorageUsage(_) = correctness {
                    println!("{msg} [Missed field: {:?}]", correctness);
                }
                if let MigrationCheckResult::StatisticsCounter(_) = correctness {
                    println!("{msg} [Missed field: {:?}]", correctness);
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
            .try_to_vec()?;
            reproducible_data_for_proofs.push((migration_data.clone(), proofs_count));

            self.commit_migration(migration_data, "Proofs", proofs_count)
                .await?;

            if i + limit >= self.data.proofs.len() {
                break;
            }

            i += limit;
        }
        assert_eq!(proofs_count, self.data.proofs.len());
        println!();

        // Accounts migration
        let mut accounts: HashMap<AccountId, Balance> = HashMap::new();
        let mut accounts_count = 0;
        let mut reproducible_data_for_accounts: Vec<(Vec<u8>, usize)> = vec![];

        for (i, (account, amount)) in self.data.accounts.iter().enumerate() {
            accounts.insert(account.clone(), amount.as_u128());

            if accounts.len() < limit && i < self.data.accounts.len() - 1 {
                continue;
            }
            accounts_count += &accounts.len();

            let migration_data = MigrationInputData {
                accounts: accounts.clone(),
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
            accounts.clear();
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

        println!("\n\n[Check correctness]");
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
        self.check_migration("Contract data:", contract_migration_data, 1)
            .await?;

        println!();
        Ok(())
    }

    /// Prepare indexed data for migration from Indexer data
    /// and store to file
    pub async fn prepare_indexed<P: AsRef<Path>>(input: P, output: P) -> anyhow::Result<()> {
        use crate::indexer::IndexerData;
        use crate::rpc::AURORA_CONTRACT;

        let data = std::fs::read(input)
            .map_err(|e| anyhow::anyhow!("Failed read indexer data file, {e}"))?;
        let indexer_data: IndexerData = IndexerData::try_from_slice(&data)
            .map_err(|e| anyhow::anyhow!("Failed deserialize indexed data, {e}"))?;
        let rpc = Client::new();

        let mut migration_data = StateData {
            contract_data: FungibleToken {
                total_eth_supply_on_near: NEP141Wei::new(0),
                total_eth_supply_on_aurora: NEP141Wei::new(0),
                // This value impossible to request from the contract
                account_storage_usage: 0,
            },
            accounts: HashMap::new(),
            accounts_counter: 0,
            proofs: vec![],
        };

        let data = rpc
            .request_view(AURORA_CONTRACT, "get_accounts_counter".to_string(), vec![])
            .await?;
        migration_data.accounts_counter = u64::try_from_slice(&data).unwrap();

        let data = rpc
            .request_view(AURORA_CONTRACT, "ft_total_supply".to_string(), vec![])
            .await?;
        let total_supply: U128 = serde_json::from_slice(&data).unwrap();
        migration_data.contract_data.total_eth_supply_on_near = NEP141Wei::new(total_supply.0);

        for account in indexer_data.data.accounts {
            let args = json!({ "account_id": account })
                .to_string()
                .as_bytes()
                .to_vec();

            let data = rpc
                .request_view(AURORA_CONTRACT, "ft_balance_of".to_string(), args)
                .await?;
            let balance: U128 =
                serde_json::from_slice(&data[..]).expect("Failed deserialize account balance");
            let account_id = AccountId::new(account.as_str()).expect("Failed deserialize account");
            migration_data
                .accounts
                .insert(account_id, NEP141Wei::new(balance.0));
            tokio::time::sleep(REQUEST_TIMEOUT).await;
        }

        for proof in indexer_data.data.proofs {
            migration_data.proofs.push(proof);
        }

        println!("Proofs: {:?}", migration_data.proofs.len());
        println!("Accounts: {:?}", migration_data.accounts.len());
        println!("Accounts counter: {:?}", migration_data.accounts_counter);
        println!(
            "Total supply: {:?}",
            migration_data
                .contract_data
                .total_eth_supply_on_near
                .as_u128()
        );

        migration_data
            .try_to_vec()
            .and_then(|data| std::fs::write(output, data))
            .map_err(|e| anyhow::anyhow!("Failed save migration data, {e}"))
    }
}
