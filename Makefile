clippy:
	@cargo clippy -- -D warnings

build-release:
	@cargo build --release
	
run:
	@cargo run --release
	
migrate: build-release
	@target/release/aurora-engine-migration-tool migrate --account ${ACCOUNT_ID} --key ${ACCOUNT_KEY}  --file contract_state.borsh
	
index: build-release
	@target/release/aurora-engine-migration-tool  indexer --block 79370015
	