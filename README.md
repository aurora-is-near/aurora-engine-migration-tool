# Aurora Engine migration tools

[![Project license](https://img.shields.io/badge/License-Public%20Domain-blue.svg)](https://creativecommons.org/publicdomain/zero/1.0/)
[![Build](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/builds.yml/badge.svg)](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/builds.yml)
[![Lints](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/lints.yml/badge.svg)](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/lints.yml)


Migration tools for [Aurora Engine](https://github.com/aurora-is-near/aurora-engine) contract. 

It is a set of tools for parsing, indexing, preparing migration, and 
migrating data from the [Aurora Engine](https://github.com/aurora-is-near/aurora-engine) contract 
to the new [aurora-eth-connector](https://github.com/aurora-is-near/aurora-eth-connector) contract.

### The set of tools includes

- `parser` - parse for Aurora Engine state snapshot
- `indexer` - indexing NEAR blockchain blocks for Aurora Engine contract
- `prepare-migrate-indexed` - prepare data for migration from indexed data
- `migration` - migrate Aurora Engine contract NEP-141 state to `aurora-eth-connector` contract.
- `CLI` - commands and parameters to interact with the application.

# How it works

## Parser

In order to parse data, it is generally necessary to have a state 
snapshot of the Aurora Engine contract. This snapshot should reflect a 
certain state of the contract at a particular point in time. The 
snapshot must be provided as a json file.

Parsing essentially does the following - it collects all existing 
accounts and their balances, as well as proof key records. Other data 
are not significant.

**What is this data for?** To transfer the state of accounts and their 
balances, as well as deposit proofs from the `Aurora Engine` contract 
to the new `aurora-eth-connector` contract. And in this case, parsing 
`Aurora Engine` state snapshot collects the necessary data and writes 
it to the resulting file, serializing it using `borsh`.

Example:

```
$ aurora-engine-migration-tool parse --file engine-snaphot-2023-05-03-120132.json -o result_file.borsh
```


### Migration

Parameters:
- `--account` - contract name for migration. Ex: `some-acc.testnet`.
- `--key` - Account private key for sign migration transactions.
- `--file` - input file that contain borsh serialized data for the migration.

Example:

```
$ aurora-engine-migration-tool migrate --account ${ACCOUNT_ID} --key ${ACCOUNT_KEY} --file contract_state.borsh
```

# Features flags

This set of tools can be used for both NEAR `mainten` 
and `testnet`. It is important to specify the 
appropriate flag explicitly.

Available options:

- `mainnet` - NEAR mainnet.
- `mainnet-archival` - NEAR mainnet-archival (after 250000 blocks from current RPC should call `archival` data).
- `testnet` - NEAR testnet.
- `log` - show log data in application output.


## Useful commands

- `make check` - run cargo `fmt` and `clippy` for all features (default command).
- `make build-mainnet-release` - build release version of application for NEAR mainnet. 
- `make build-testnet-release` - build release version of application for NEAR testnet.
- `migrate-testnet` - build testnet release and run migration with parameters from environment:
    - `--account ${ACCOUNT_ID}` - contract name for migration. Ex: `some-acc.testnet`.
    - `--key ${ACCOUNT_KEY}` - Account private key for sign migration transactions. 
    - `--file contract_state.borsh` - input data for migration.
- `make index-fullstat` - build indexer and run full-stat command for indexer.
- `index-stat`- build indexer and run short statistics for indexer.
- `index-history` - build indexer and run indexing historical data.
- `make prepare-migration` - build indexer and run data preparation with params:
  - `-f data.borsh` - input prepared data (for example from indexer)
  - `-o for-migtation.borsh` - output file, that can be used to run migration.


### LICENSE: [CC0 1.0 Universal](LICENSE)