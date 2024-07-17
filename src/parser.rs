use aurora_engine_migration_tool::{BlockData, FungibleToken, StateData};
use aurora_engine_types::storage::{bytes_to_key, EthConnectorStorageId, KeyPrefix};
use aurora_engine_types::types::NEP141Wei;
use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::AccountId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

enum KeyType {
    Accounts(Vec<u8>),
    Contract,
    Unknown,
}

pub fn construct_contract_key(suffix: EthConnectorStorageId) -> Vec<u8> {
    bytes_to_key(KeyPrefix::EthConnector, &[u8::from(suffix)])
}

pub fn prefix_account_key() -> Vec<u8> {
    bytes_to_key(
        KeyPrefix::EthConnector,
        &[u8::from(EthConnectorStorageId::FungibleToken)],
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

    let mut accounts: HashMap<AccountId, NEP141Wei> = HashMap::new();
    let mut contract_data: FungibleToken = FungibleToken::default();
    let mut total_stuck_supply = NEP141Wei::new(0);

    for result_value in &json_data.result.values {
        let key = base64::decode(&result_value.key)
            .map_err(|e| anyhow::anyhow!("Failed deserialize key, {e}"))?;
        // Get proofs
        match key_type(&key) {
            KeyType::Accounts(value) => {
                let account_str = std::str::from_utf8(&value)
                    .map_err(|e| anyhow::anyhow!("Failed parse account to str, {e}"))?;
                let account_balance = NEP141Wei::try_from_slice(
                    &base64::decode(&result_value.value)
                        .map_err(|e| anyhow::anyhow!("Failed get account balance, {e}"))?,
                )
                .map_err(|e| anyhow::anyhow!("Failed parse account balance, {e}"))?;
                let Ok(account) = AccountId::from_str(account_str) else {
                    total_stuck_supply = total_stuck_supply + account_balance;
                    println!("\tNot fetched account: {account_str} with balance {account_balance}");
                    continue;
                };
                accounts.insert(account, account_balance);
            }
            KeyType::Contract => {
                let val = base64::decode(&result_value.value)
                    .map_err(|e| anyhow::anyhow!("Failed get contract data, {e}"))?;

                contract_data = FungibleToken::try_from_slice(&val)
                    .map_err(|e| anyhow::anyhow!("Failed parse contract data, {e}"))?;
            }
            KeyType::Unknown => (), //anyhow::bail!("Unknown key type"),
        }
    }
    println!("Accounts: {}", accounts.len());
    println!("Total supply: {}", contract_data.total_eth_supply_on_near);
    println!("Total stuck supply: {}", total_stuck_supply);

    // Store result data
    StateData {
        total_supply: contract_data.total_eth_supply_on_near,
        total_stuck_supply,
        accounts,
    }
    .try_to_vec()
    .and_then(|data| std::fs::write(result_file_name, data))
    .map_err(|e| anyhow::anyhow!("Failed save result data, {e}"))
}

fn key_type(key: &[u8]) -> KeyType {
    if is_account_prefix_key(key) {
        let account_prefix_len = prefix_account_key().len();
        let value = key[account_prefix_len..].to_vec();
        KeyType::Accounts(value)
    } else if key == get_contract_key() {
        KeyType::Contract
    } else {
        KeyType::Unknown
    }
}

fn is_account_prefix_key(key: &[u8]) -> bool {
    let account_prefix = &prefix_account_key();
    key.len() > account_prefix.len() && &key[..account_prefix.len()] == account_prefix
}
