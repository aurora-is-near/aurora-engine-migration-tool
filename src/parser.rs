use aurora_engine_migration_tool::{BlockData, FungibleToken, StateData};
use aurora_engine_types::storage::{EthConnectorStorageId, KeyPrefix, VersionPrefix};
use aurora_engine_types::types::NEP141Wei;
use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::AccountId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

enum KeyType {
    Accounts(Vec<u8>),
    Contract,
    Proof(Vec<u8>),
    Statistic,
    Unknown,
}

pub fn bytes_to_key(prefix: KeyPrefix, bytes: &[u8]) -> Vec<u8> {
    [&[u8::from(VersionPrefix::V1)], &[u8::from(prefix)], bytes].concat()
}

pub fn construct_contract_key(suffix: EthConnectorStorageId) -> Vec<u8> {
    bytes_to_key(KeyPrefix::EthConnector, &[u8::from(suffix)])
}

pub fn read_u64(value: &[u8]) -> u64 {
    assert_eq!(value.len(), 8, "Failed parse u64");

    let mut result = [0u8; 8];
    result.copy_from_slice(value.as_ref());
    u64::from_le_bytes(result)
}

pub fn prefix_proof_key() -> Vec<u8> {
    construct_contract_key(EthConnectorStorageId::UsedEvent)
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
    construct_contract_key(EthConnectorStorageId::FungibleToken)
}

pub fn parse<P: AsRef<Path>>(json_file: P, output: Option<P>) -> anyhow::Result<()> {
    let data =
        std::fs::read_to_string(json_file).map_err(|e| anyhow::anyhow!("Failed read data: {e}"))?;
    let json_data: BlockData =
        serde_json::from_str(&data).map_err(|e| anyhow::anyhow!("Failed read json: {e}"))?;
    let result_file_name = output.map_or_else(
        || {
            PathBuf::from(format!(
                "contract_state{}.borsh",
                json_data.result.block_height
            ))
        },
        |p| p.as_ref().to_path_buf(),
    );

    println!("Block height: {:?}", json_data.result.block_height);
    println!("Data size: {:.3} Gb", data.len() as f64 / 1_000_000_000.);
    println!("Data values: {:#?}", json_data.result.values.len());

    let mut proofs: Vec<String> = vec![];
    let mut accounts: HashMap<AccountId, NEP141Wei> = HashMap::new();
    let mut accounts_counter: u64 = 0;
    let mut contract_data: FungibleToken = FungibleToken::default();

    for result_value in &json_data.result.values {
        let key = base64::decode(&result_value.key)
            .map_err(|e| anyhow::anyhow!("Failed deserialize key, {e}"))?;
        // Get proofs
        match key_type(&key) {
            KeyType::Proof(value) => {
                let proof = String::from_utf8(value)
                    .map_err(|e| anyhow::anyhow!("Failed parse proof, {e}"))?;
                proofs.push(proof);
            }
            KeyType::Accounts(value) => {
                let account = AccountId::try_from_slice(value.as_slice())
                    .map_err(|e| anyhow::anyhow!("Failed parse account, {e}"))?;
                let account_balance = NEP141Wei::try_from_slice(
                    &base64::decode(&result_value.value)
                        .map_err(|e| anyhow::anyhow!("Failed get account balance, {e}"))?,
                )
                .map_err(|e| anyhow::anyhow!("Failed parse account balance, {e}"))?;
                accounts.insert(account, account_balance);
            }
            KeyType::Contract => {
                let val = base64::decode(&result_value.value)
                    .map_err(|e| anyhow::anyhow!("Failed get contract data, {e}"))?;
                contract_data = FungibleToken::try_from_slice(&val)
                    .map_err(|e| anyhow::anyhow!("Failed parse contract data, {e}"))?;
            }
            KeyType::Statistic => {
                let val = base64::decode(&result_value.value)
                    .map_err(|e| anyhow::anyhow!("Failed get account counter, {e}"))?;
                accounts_counter = read_u64(&val);
            }
            KeyType::Unknown => anyhow::bail!("Unknown key type"),
        }
    }
    println!("Proofs: {}", proofs.len());
    println!("Accounts: {}", accounts.len());
    assert_eq!(
        accounts.len() as u64,
        accounts_counter,
        "Wrong accounts count"
    );
    // Store result data
    println!("Result file: {result_file_name:?}");
    StateData {
        contract_data,
        accounts,
        accounts_counter,
        proofs,
    }
    .try_to_vec()
    .and_then(|data| std::fs::write(result_file_name, data))
    .map_err(|e| anyhow::anyhow!("Failed save result data, {e}"))
}

fn key_type(key: &[u8]) -> KeyType {
    if is_prefix_proof_key(key) {
        let proof_prefix_len = prefix_proof_key().len();
        let value = key[proof_prefix_len..].to_vec();
        KeyType::Proof(value)
    } else if is_account_prefix_key(key) {
        let account_prefix_len = prefix_account_key().len();
        let value = key[account_prefix_len..].to_vec();
        KeyType::Accounts(value)
    } else if key == get_contract_key() {
        KeyType::Contract
    } else if key == get_statistic_key() {
        KeyType::Statistic
    } else {
        KeyType::Unknown
    }
}

fn is_prefix_proof_key(key: &[u8]) -> bool {
    let proof_prefix = &prefix_proof_key();
    key.len() > proof_prefix.len() && &key[..proof_prefix.len()] == proof_prefix
}

fn is_account_prefix_key(key: &[u8]) -> bool {
    let account_prefix = &prefix_account_key();
    key.len() > account_prefix.len() && &key[..account_prefix.len()] == account_prefix
}
