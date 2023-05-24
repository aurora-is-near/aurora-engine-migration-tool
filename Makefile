BIN=target/release/aurora-engine-migration-tool

check:
	@cargo fmt -- --check
	@cargo clippy --features mainnet -- -D warnings  -D clippy::pedantic -A clippy::too-many-lines -A clippy::missing_panics_doc -A clippy::missing_errors_doc -A clippy::cast-precision-loss -A clippy::module_name_repetitions
	@cargo clippy --features mainnet-archival -- -D warnings  -D clippy::pedantic -A clippy::too-many-lines -A clippy::missing_panics_doc -A clippy::missing_errors_doc -A clippy::cast-precision-loss -A clippy::module_name_repetitions
	@cargo clippy --features testnet -- -D warnings  -D clippy::pedantic -A clippy::too-many-lines -A clippy::missing_panics_doc -A clippy::missing_errors_doc -A clippy::cast-precision-loss -A clippy::module_name_repetitions

build-mainnet-release:
	@cargo build --features mainnet --release

build-testnet-release:
	@cargo build --features testnet --release

run: build-mainnet-release
	@${BIN} indexer --help

migrate-testnet: build-testnet-release
	@${BIN} migrate --account ${ACCOUNT_ID} --key ${ACCOUNT_KEY} --file contract_state.borsh

migrate-mainnet: build-mainnet-release
	@${BIN} migrate --account ${ACCOUNT_ID} --key ${ACCOUNT_KEY} --file contract_state.borsh

build-index-archival:
	@cargo build --release --features mainnet-archival

build-index:
	@cargo build --release --features mainnet
	
index-block: build-index-archival
#	@${BIN} indexer --block 79373253
#	@${BIN} indexer --block 79377726
#	@${BIN} indexer --block 82952720
	@${BIN} indexer --force --block 83889386

index-latest: build-index
	@${BIN} indexer 

index-history: build-index-archival
	@${BIN} indexer -H

index-stat: build-index
	@${BIN} indexer --stat

index-fullstat: build-index
	@${BIN} indexer --fullstat

prepare-migration: build-index
	@${BIN} prepare-migrate-indexed -f data.borsh -o for-migtation.borsh
