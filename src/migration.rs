use crate::rpc::RPC;
use aurora_engine_migration_tool::StateData;
use aurora_engine_types::types::NEP141Wei;
use borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::{AccountId, StorageUsage};
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
    pub accounts_eth: HashMap<AccountId, NEP141Wei>,
    pub total_eth_supply_on_near: Option<NEP141Wei>,
    pub account_storage_usage: Option<StorageUsage>,
    pub statistics_aurora_accounts_counter: Option<u64>,
    pub used_proofs: Vec<String>,
}

impl Migration {
    pub async fn new(
        data_file: &PathBuf,
        signer_account_id: String,
        signer_secret_key: String,
        contract: String,
    ) -> anyhow::Result<Self> {
        let data = std::fs::read(data_file).unwrap_or_default();
        let data: StateData = StateData::try_from_slice(&data[..]).expect("Failed parse data");

        println!("{} [{}] {}", signer_account_id, signer_secret_key, contract);

        Ok(Self {
            rpc: RPC::new().await?,
            data,
            config: MigrationConfig {
                signer_account_id,
                signer_secret_key,
                contract,
            },
        })
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let migration_data = MigrationInputData {
            accounts_eth: HashMap::new(),
            statistics_aurora_accounts_counter: Some(10),
            total_eth_supply_on_near: Some(NEP141Wei::new(20)),
            account_storage_usage: Some(30),
            used_proofs: vec![],
        }
        .try_to_vec()
        .expect("Failed to parse migration data");
        self.rpc
            .commit_tx(
                self.config.signer_account_id.clone(),
                self.config.signer_secret_key.clone(),
                self.config.contract.clone(),
                MIGRATION_METHOD.to_string(),
                migration_data,
            )
            .await?;
        Ok(())
    }
}
/*
async fn rpc() -> anyhow::Result<bool> {
    use near_jsonrpc_client::{methods, JsonRpcClient};
    use near_jsonrpc_primitives::types::query::QueryResponseKind;
    use near_primitives::types::{BlockReference, Finality, FunctionArgs};
    use near_primitives::views::QueryRequest;
    use near_sdk::json_types::U128;
    use serde_json::{from_slice, json};

    let contract_acc = env!("ENV_ACC");
    println!("Contract: {} [{}]", contract_acc, env!("ENV_PK"));

    let client = JsonRpcClient::connect("https://rpc.testnet.near.org");
    let request = methods::query::RpcQueryRequest {
        block_reference: BlockReference::Finality(Finality::Final),
        request: QueryRequest::CallFunction {
            account_id: contract_acc.parse()?,
            method_name: "ft_total_supply".to_string(),
            args: FunctionArgs::from(json!({}).to_string().into_bytes()),
        },
    };

    let response = client.call(request).await?;

    if let QueryResponseKind::CallResult(result) = response.kind {
        println!("{:#?}", from_slice::<U128>(&result.result)?.0);
    }
    Ok(true)
}
*/
