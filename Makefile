clippy:
	@cargo clippy -- -D warnings
release:
	@cargo build --release
	
run:
	@cargo run --release