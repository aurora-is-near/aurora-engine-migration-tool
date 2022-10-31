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
    construct_contract_key(&EthConnectorStorageId::UsedEvent).to_vec()
}

fn main() {
    println!(
        "Aurora Engine migration tool v{}",
        env!("CARGO_PKG_VERSION")
    );
    let json_file = args().nth(1).expect("Expected json file");
    let data = std::fs::read_to_string(json_file).expect("Failed read data");
    let json_data: BlockData = serde_json::from_str(&data).expect("Failed read json");
    println!("Data size: {:.3} Gb", data.len() as f64 / 1_000_000_000.);
    println!("Data values: {:#?}", json_data.result.values.len());

    let proof_prefix = &prefix_proof_key()[..];
    let mut proofs: Vec<String> = vec![];
    for value in &json_data.result.values {
        let key = base64::decode(&value.key).expect("Failed deserialize key");
        if key.len() > proof_prefix.len() && &key[..proof_prefix.len()] == proof_prefix {
            let val = key[proof_prefix.len()..].to_vec();
            let proof = String::from_utf8(val).unwrap();
            proofs.push(proof);
        }
    }
    println!("Proofs: {:?}", proofs.len());
}
