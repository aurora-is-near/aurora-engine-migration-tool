use aurora_engine_types::account_id::AccountId;
use aurora_engine_types::storage::{EthConnectorStorageId, KeyPrefix, VersionPrefix};
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
    let mut proofs: Vec<String> = vec![];
    let mut accounts: Vec<AccountId> = vec![];
    for value in &json_data.result.values {
        let key = base64::decode(&value.key).expect("Failed deserialize key");
        // Get proofs
        if key.len() > proof_prefix.len() && &key[..proof_prefix.len()] == proof_prefix {
            let val = key[proof_prefix.len()..].to_vec();
            let proof = String::from_utf8(val).expect("Failed parse proof");
            proofs.push(proof);
        }
        // Get accounts
        if key.len() > account_prefix.len() && &key[..account_prefix.len()] == account_prefix {
            let val = key[account_prefix.len()..].to_vec();
            let account =
                AccountId::try_from(String::from_utf8(val).expect("Failed parse account"))
                    .expect("Failed parse account");
            accounts.push(account);
        }
    }
    println!("Proofs: {:?}", proofs.len());
    println!("Accounts: {:?}", accounts.len());
}
