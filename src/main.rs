use aurora_engine_types::account_id::AccountId;
use aurora_engine_types::storage::{EthConnectorStorageId, KeyPrefix, VersionPrefix};
// use aurora_engine_types::types::NEP141Wei;
use serde_derive::Deserialize;
use std::env::args;

#[derive(Deserialize, Debug)]
pub struct ResultValues {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize, Debug)]
pub struct ResultData {
    pub block_height: u64,
    pub values: Vec<ResultValues>,
}

#[derive(Deserialize, Debug)]
pub struct BlockData {
    pub result: ResultData,
}

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

fn main() {
    println!(
        "Aurora Engine migration tool v{}",
        env!("CARGO_PKG_VERSION")
    );
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
    let mut accounts: Vec<AccountId> = vec![];
    let mut accounts_counter: u64 = 0;
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
            accounts.push(account);
            continue;
        }
        // Account statistics
        if key == account_counter_key {
            let val = base64::decode(&value.value).expect("Failed get account counter");
            accounts_counter = read_u64(&val[..]);
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
    println!("Accounts counter: {:?}", accounts_counter);
}
