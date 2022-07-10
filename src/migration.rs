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
