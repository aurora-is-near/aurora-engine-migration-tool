use crate::rpc::RPC;
use aurora_engine_migration_tool::StateData;
use aurora_engine_types::types::NEP141Wei;
use borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::{AccountId, Balance, StorageUsage};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;

const MIGRATION_METHOD: &str = "migrate";

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
        #[derive(BorshSerialize, BorshDeserialize)]
        pub struct MData {
            pub accounts: HashMap<AccountId, Balance>,
            pub total_supply: Option<Balance>,
            pub account_storage_usage: Option<StorageUsage>,
            pub statistics_aurora_accounts_counter: Option<u64>,
            pub used_proofs: Vec<String>,
        }

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

        let res = MigrationCheckResult::try_from_slice(&res[..]);
        println!("{:?}", res);

        Ok(())
    }
}
