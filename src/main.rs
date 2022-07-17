use aurora_engine_migration_tool::{BlockData, FungibleToken, StateData};
use aurora_engine_types::account_id::AccountId;
use aurora_engine_types::storage::{EthConnectorStorageId, KeyPrefix, VersionPrefix};
use aurora_engine_types::types::NEP141Wei;
use aurora_engine_types::HashMap;
use borsh::{BorshDeserialize, BorshSerialize};
use near_primitives::types::BlockHeight;
use near_primitives::views::ActionView;
use std::collections::HashSet;
use std::env::args;
use std::io::Write;
use std::time::Duration;

pub mod rpc;

pub fn bytes_to_key(prefix: KeyPrefix, bytes: &[u8]) -> Vec<u8> {
    [&[u8::from(VersionPrefix::V1)], &[u8::from(prefix)], bytes].concat()
}

pub fn construct_contract_key(suffix: &EthConnectorStorageId) -> Vec<u8> {
    bytes_to_key(KeyPrefix::EthConnector, &[u8::from(*suffix)])
}

pub fn prefix_proof_key() -> Vec<u8> {
    construct_contract_key(&EthConnectorStorageId::UsedEvent)
}

pub fn prefix_account_key() -> Vec<u8> {
    bytes_to_key(
        KeyPrefix::EthConnector,
        &[u8::from(EthConnectorStorageId::FungibleToken)],
    )
}

pub fn get_statistic_key() -> Vec<u8> {
    bytes_to_key(
        KeyPrefix::EthConnector,
        &[u8::from(
            EthConnectorStorageId::StatisticsAuroraAccountsCounter,
        )],
    )
}

pub fn read_u64(value: &[u8]) -> u64 {
    if value.len() != 8 {
        panic!("Failed parse u64")
    }
    let mut result = [0u8; 8];
    result.copy_from_slice(value.as_ref());
    u64::from_le_bytes(result)
}

pub fn get_contract_key() -> Vec<u8> {
    construct_contract_key(&EthConnectorStorageId::FungibleToken)
}

async fn indexer() -> anyhow::Result<()> {
    use near_jsonrpc_client::{methods, JsonRpcClient};

    let block_reference =
        near_primitives::types::BlockReference::Finality(near_primitives::types::Finality::Final);

    let client = JsonRpcClient::connect("https://rpc.mainnet.near.org");
    let mut block_height_pool: HashSet<BlockHeight> = HashSet::new();
    let block = client
        .call(methods::block::RpcBlockRequest {
            block_reference: block_reference.clone(),
        })
        .await
        .expect("Failed get latest block");
    let final_height: u64 = block.header.height;
    let mut height: u64 = 79372150; // final_height;
    let block_limit = 10_000_000;

    println!("Starting height: {:?}", height);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;

        print!("\rHeight: {:?}", height);
        std::io::stdout().flush().expect("Flush failed");

        let block_reference = near_primitives::types::BlockReference::BlockId(
            near_primitives::types::BlockId::Height(height),
        );
        let chunks = if let Ok(block_res) = client
            .call(methods::block::RpcBlockRequest { block_reference })
            .await
        {
            block_res.chunks
        } else {
            println!("\nFailed get block: {:?}", height);
            continue;
        };
        for chunk in chunks {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Ok(chunk_res) = client
                .call(methods::chunk::RpcChunkRequest {
                    chunk_reference:
                        near_jsonrpc_primitives::types::chunks::ChunkReference::ChunkHash {
                            chunk_id: chunk.chunk_hash,
                        },
                })
                .await
            {
                if !chunk_res.transactions.is_empty() {
                    for tx in &chunk_res.transactions {
                        if tx.receiver_id.as_str() == "aurora" {
                            for action in &tx.actions {
                                if let ActionView::FunctionCall { method_name, .. } = action {
                                    if method_name == "submit" {
                                        continue;
                                    }
                                    tokio::time::sleep(Duration::from_millis(50)).await;

                                    println!(
                                        "\n{:?} [{:?}]: {:?}",
                                        height, chunk.shard_id, method_name
                                    );
                                    if let Ok(tx_info) = client
                                        .call(methods::tx::RpcTransactionStatusRequest {
                                            transaction_info:
                                                methods::tx::TransactionInfo::TransactionId {
                                                    hash: tx.hash,
                                                    account_id: tx.signer_id.clone(),
                                                },
                                        })
                                        .await
                                    {
                                        println!("\nTx: {:#?}", tx_info.status);
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                println!("\nFailed get chunk for height: {:?}", height);
                block_height_pool.insert(height);
            }
        }
        height -= 1;
        if height <= final_height - block_limit {
            break;
        }
    }
    Ok(())
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!(
        "Aurora Engine migration tool v{}",
        env!("CARGO_PKG_VERSION")
    );
    indexer().await?;
    // if rpc().await.unwrap() {
    //     return Ok(());
    // }

    let json_file = args().nth(1).expect("Expected json file");
    let data = std::fs::read_to_string(json_file).expect("Failed read data");
    let json_data: BlockData = serde_json::from_str(&data).expect("Failed read json");
    println!("Block height: {:?}", json_data.result.block_height);
    println!("Data size: {:.3} Gb", data.len() as f64 / 1_000_000_000.);
    println!("Data values: {:#?}", json_data.result.values.len());

    let proof_prefix = &prefix_proof_key()[..];
    let account_prefix = &prefix_account_key()[..];
    let account_counter_key = &get_statistic_key()[..];
    let mut proofs: Vec<String> = vec![];
    let mut accounts: HashMap<AccountId, NEP141Wei> = HashMap::new();
    let mut accounts_counter: u64 = 0;
    let mut contract_data: FungibleToken = FungibleToken::default();
    for value in &json_data.result.values {
        let key = base64::decode(&value.key).expect("Failed deserialize key");
        // Get proofs
        if key.len() > proof_prefix.len() && &key[..proof_prefix.len()] == proof_prefix {
            let val = key[proof_prefix.len()..].to_vec();
            let proof = String::from_utf8(val).expect("Failed parse proof");
            proofs.push(proof);
            continue;
        }
        // Get accounts
        if key.len() > account_prefix.len() && &key[..account_prefix.len()] == account_prefix {
            let val = key[account_prefix.len()..].to_vec();
            let account =
                AccountId::try_from(String::from_utf8(val).expect("Failed parse account"))
                    .expect("Failed parse account");
            let account_balance = NEP141Wei::try_from_slice(
                &base64::decode(&value.value).expect("Failed get account balance")[..],
            )
            .expect("Failed parse account balance");
            accounts.insert(account, account_balance);
            continue;
        }
        // Account statistics
        if key == account_counter_key {
            let val = base64::decode(&value.value).expect("Failed get account counter");
            accounts_counter = read_u64(&val[..]);
            continue;
        }
        // Get contract data
        if key == get_contract_key() {
            let val = &base64::decode(&value.value).expect("Failed get contract data")[..];
            contract_data = FungibleToken::try_from_slice(val).expect("Failed parse contract data");
            continue;
        }
    }
    println!("Proofs: {:?}", proofs.len());
    println!("Accounts: {:?}", accounts.len());
    assert_eq!(
        accounts.len() as u64,
        accounts_counter,
        "Wrong accounts count"
    );
    // Store result data
    let data = StateData {
        contract_data,
        accounts,
        accounts_counter,
        proofs,
    }
    .try_to_vec()
    .expect("Failed serialize data");

    let file_name = format!("contract_state{:?}.borsh", json_data.result.block_height);
    println!("Result file: {}", file_name);
    std::fs::write(file_name, data).expect("Failed save result data");

    Ok(())
}
