use aurora_engine_migration_tool::{BlockData, FungibleToken, StateData};
use aurora_engine_types::account_id::AccountId;
use aurora_engine_types::storage::{EthConnectorStorageId, KeyPrefix, VersionPrefix};
use aurora_engine_types::types::NEP141Wei;
use aurora_engine_types::HashMap;
use borsh::{BorshDeserialize, BorshSerialize};
use std::path::PathBuf;

pub fn bytes_to_key(prefix: KeyPrefix, bytes: &[u8]) -> Vec<u8> {
    [&[u8::from(VersionPrefix::V1)], &[u8::from(prefix)], bytes].concat()
}

pub fn construct_contract_key(suffix: &EthConnectorStorageId) -> Vec<u8> {
    bytes_to_key(KeyPrefix::EthConnector, &[u8::from(*suffix)])
}

pub fn read_u64(value: &[u8]) -> u64 {
    if value.len() != 8 {
        panic!("Failed parse u64")
    }
    let mut result = [0u8; 8];
    result.copy_from_slice(value.as_ref());
    u64::from_le_bytes(result)
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

pub fn get_contract_key() -> Vec<u8> {
    construct_contract_key(&EthConnectorStorageId::FungibleToken)
}

pub fn parse(json_file: &PathBuf, output: Option<PathBuf>) {
    let data = std::fs::read_to_string(json_file).expect("Failed read data");
    let json_data: BlockData = serde_json::from_str(&data).expect("Failed read json");
    let result_file_name: PathBuf = if let Some(output) = output {
        output
    } else {
        use std::str::FromStr;

        PathBuf::from_str(&format!(
            "contract_state{:?}.borsh",
            json_data.result.block_height
        ))
        .expect("Failed parse output result file")
    };

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

    println!("Result file: {:?}", result_file_name);
    std::fs::write(result_file_name, data).expect("Failed save result data");
}
