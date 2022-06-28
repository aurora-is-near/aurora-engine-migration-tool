use crate::rpc::RPC;
use aurora_engine_migration_tool::StateData;
use borsh::BorshDeserialize;
use std::path::PathBuf;

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
impl Migration {
    pub async fn new(
        data_file: &PathBuf,
        signer_account_id: String,
        signer_secret_key: String,
        contract: String,
    ) -> anyhow::Result<Self> {
        let data = std::fs::read(data_file).unwrap_or_default();
        let data: StateData = StateData::try_from_slice(&data[..]).expect("Failed parse data");

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
