pub use aurora_engine_types::account_id::AccountId;
pub use aurora_engine_types::types::{NEP141Wei, StorageUsage};
use aurora_engine_types::HashMap;
pub use borsh::{BorshDeserialize, BorshSerialize};
use serde_derive::Deserialize;

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

#[derive(Debug, Default, BorshDeserialize, BorshSerialize)]
pub struct FungibleToken {
    pub total_eth_supply_on_near: NEP141Wei,
    pub total_eth_supply_on_aurora: NEP141Wei,
    pub account_storage_usage: StorageUsage,
}

#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct StateData {
    pub contract_data: FungibleToken,
    pub accounts: HashMap<AccountId, NEP141Wei>,
    pub accounts_counter: u64,
    pub proofs: Vec<String>,
}
