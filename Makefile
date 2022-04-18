check:
	@cargo fmt -- --check
	@cargo clippy --no-default-features --features mainnet -- -D warnings
	@cargo clippy --no-default-features --features testnet -- -D warnings

build-release:
	@cargo build --release
	
run:
	@cargo run --release
	
migrate: build-release
	@target/release/aurora-engine-migration-tool migrate --account ${ACCOUNT_ID} --key ${ACCOUNT_KEY}  --file contract_state.borsh

build-index:
	@cargo build --release --no-default-features --features mainnet
	
index: build-index
	@target/release/aurora-engine-migration-tool  indexer --block 79373253
#	@target/release/aurora-engine-migration-tool  indexer --block 79377726

index-latest: build-index
	@target/release/aurora-engine-migration-tool  indexer 
