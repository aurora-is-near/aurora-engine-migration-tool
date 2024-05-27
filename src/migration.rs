use crate::rpc::{Client, REQUEST_TIMEOUT};
use aurora_engine_migration_tool::StateData;
use aurora_engine_types::types::NEP141Wei;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::{AccountId, Balance};
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
}

#[derive(Debug, BorshSerialize, BorshDeserialize, Eq, PartialEq)]
pub enum MigrationCheckResult {
    Success,
    AccountNotExist(Vec<AccountId>),
    AccountAmount(HashMap<AccountId, Balance>),
    TotalSupply(Balance),
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
            MigrationCheckResult::AccountNotExist(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len());
            }
            MigrationCheckResult::AccountAmount(missed) => {
                println!("{msg}: {counter} [Missed: {:?}]", missed.len());
            }
            MigrationCheckResult::Success => {
                print!("\r{msg}: {counter} [{correctness:?}]");
                std::io::stdout().flush()?;
            }
            MigrationCheckResult::TotalSupply(_) => {
                println!("{msg} [Missed field: {correctness:?}]");
            }
        }
        Ok(())
    }

    // Checking the correctness and integrity of data, regardless of
    // the migration process
    async fn check_migration_full(&self, reproducible_data_for_accounts:  Vec<(HashMap<AccountId, Balance>, usize)>) -> anyhow::Result<()> {
        println!();
        for (accounts, counter) in reproducible_data_for_accounts {
            let migration_data = MigrationInputData {
                accounts: accounts.clone(),
                total_supply: None,
            }
                .try_to_vec()
                .expect("Failed serialize");

            self.check_migration("Accounts:", migration_data, counter)
                .await?;
        }

        println!();
        let contract_migration_data = MigrationInputData {
            accounts: HashMap::new(),
            total_supply: Some(
                self.data.total_supply.as_u128() - self.data.total_stuck_supply.as_u128(),
            ),
        }
            .try_to_vec()
            .expect("Failed serialize");
        self.check_migration("Contract data:", contract_migration_data, 1)
            .await?;

        println!();
        Ok(())
    }

    fn get_reproducible_data_for_accounts(&self) -> Vec<(HashMap<AccountId, Balance>, usize)> {
        // Data limit per transaction
        let limit = RECORDS_COUNT_PER_TX;

        // Accounts migration
        let mut accounts: HashMap<AccountId, Balance> = HashMap::new();
        let mut accounts_count = 0;
        let mut reproducible_data_for_accounts: Vec<(HashMap<AccountId, Balance>, usize)> = vec![];

        for (i, (account, amount)) in self.data.accounts.iter().enumerate() {
            accounts.insert(account.clone(), amount.as_u128());

            if accounts.len() < limit && i < self.data.accounts.len() - 1 {
                continue;
            }
            accounts_count += &accounts.len();

            reproducible_data_for_accounts.push((accounts.clone(), accounts_count));

            // Clear
            accounts.clear();
        }

        assert_eq!(self.data.accounts.len(), accounts_count);

        reproducible_data_for_accounts
    }

    /// Check migration
    pub async fn validate_migration(&self) -> anyhow::Result<()> {
        let reproducible_data_for_accounts = self.get_reproducible_data_for_accounts();
        self.check_migration_full(reproducible_data_for_accounts).await
    }

    /// Run migration process
    pub async fn run(&self) -> anyhow::Result<()> {
        let reproducible_data_for_accounts = self.get_reproducible_data_for_accounts();
        for (accounts, accounts_count) in &reproducible_data_for_accounts {
            let migration_data: Vec<AccountId> = accounts.keys().cloned().collect();
            self.commit_migration(
                migration_data.try_to_vec().expect("Failed serialize"),
                "Accounts",
                accounts_count.clone(),
            )
            .await?;
        }

        self.check_migration_full(reproducible_data_for_accounts).await
    }

    /// Prepare indexed data for migration from Indexer data
    /// and store to file serialized with borsh.
    pub async fn prepare_indexed<P: AsRef<Path>>(input: P, output: P) -> anyhow::Result<()> {
        use crate::indexer::IndexerData;
        use crate::rpc::AURORA_CONTRACT;

        let data = std::fs::read(input)
            .map_err(|e| anyhow::anyhow!("Failed read indexer data file, {e}"))?;
        let indexer_data: IndexerData = IndexerData::try_from_slice(&data)
            .map_err(|e| anyhow::anyhow!("Failed deserialize indexed data, {e}"))?;
        let rpc = Client::new();

        let mut migration_data = StateData {
            total_supply: NEP141Wei::new(0),
            total_stuck_supply: NEP141Wei::new(0),
            accounts: HashMap::new(),
        };

        let data = rpc
            .request_view(AURORA_CONTRACT, "ft_total_supply".to_string(), vec![])
            .await?;
        let total_supply: U128 = serde_json::from_slice(&data).unwrap();
        migration_data.total_supply = NEP141Wei::new(total_supply.0);

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
            migration_data
                .accounts
                .insert(account, NEP141Wei::new(balance.0));
            tokio::time::sleep(REQUEST_TIMEOUT).await;
        }

        println!("Accounts: {:?}", migration_data.accounts.len());
        println!("Total supply: {:?}", migration_data.total_supply.as_u128());

        migration_data
            .try_to_vec()
            .and_then(|data| std::fs::write(output, data))
            .map_err(|e| anyhow::anyhow!("Failed save migration data, {e}"))
    }

    pub fn combine_indexed_and_state_data<P: AsRef<Path>>(
        state: P,
        indexed: P,
        output: P,
    ) -> anyhow::Result<()> {
        let mut state_data = {
            let data = std::fs::read(state)
                .map_err(|e| anyhow::anyhow!("Failed read state data file, {e}"))?;
            StateData::try_from_slice(&data)
                .map_err(|e| anyhow::anyhow!("Failed deserialize state data, {e}"))?
        };

        let indexed_data = {
            let data = std::fs::read(indexed)
                .map_err(|e| anyhow::anyhow!("Failed read indexed data file, {e}"))?;
            StateData::try_from_slice(&data)
                .map_err(|e| anyhow::anyhow!("Failed deserialize indexed data, {e}"))?
        };

        for (account, balance) in indexed_data.accounts {
            state_data.accounts.insert(account, balance);
        }
        state_data.total_supply = indexed_data.total_supply;

        println!("Accounts: {:?}", state_data.accounts.len());
        println!("Total supply: {:?}", state_data.total_supply.as_u128());
        println!(
            "Total stuck supply: {:?}",
            state_data.total_stuck_supply.as_u128()
        );

        state_data
            .try_to_vec()
            .and_then(|data| std::fs::write(output, data))
            .map_err(|e| anyhow::anyhow!("Failed save migration data, {e}"))
    }
}
