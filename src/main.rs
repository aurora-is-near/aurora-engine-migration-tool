use serde_derive::Deserialize;
use std::env::args;

#[derive(Deserialize, Debug)]
pub struct ResultValues {
    key: String,
    value: String,
}

#[derive(Deserialize, Debug)]
pub struct ResultData {
    values: Vec<ResultValues>,
}

#[derive(Deserialize, Debug)]
pub struct BlockData {
    result: ResultData,
}

fn main() {
    println!(
        "Aurora Engine migration tool v{}",
        env!("CARGO_PKG_VERSION")
    );
    let json_file = args().nth(1).expect("Expected json file");
    let data = std::fs::read_to_string(json_file).expect("Failed read data");
    let json_data: BlockData = serde_json::from_str(&data).expect("Failed read json");
    println!("{:.3} Gb", data.len() as f64 / 1_000_000_000.);
    println!("{:#?} items", json_data.result.values.len());
}
