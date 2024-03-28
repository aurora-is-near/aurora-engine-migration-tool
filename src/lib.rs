pub use aurora_engine_types::types::{NEP141Wei, StorageUsage};
pub use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::AccountId;
use serde_derive::Deserialize;
use std::collections::HashMap;

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
}

#[derive(Debug, Default, BorshSerialize, BorshDeserialize)]
pub struct StateData {
    pub total_supply: NEP141Wei,
    pub total_stuck_supply: NEP141Wei,
    pub accounts: HashMap<AccountId, NEP141Wei>,
}
