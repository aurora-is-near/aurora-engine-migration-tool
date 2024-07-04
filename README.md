# Aurora Engine migration tools

[![Project license](https://img.shields.io/badge/License-Public%20Domain-blue.svg)](https://creativecommons.org/publicdomain/zero/1.0/)
[![Build](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/builds.yml/badge.svg)](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/builds.yml)
[![Lints](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/lints.yml/badge.svg)](https://github.com/aurora-is-near/aurora-engine-migration-tool/actions/workflows/lints.yml)


Migration tools for [Aurora Engine](https://github.com/aurora-is-near/aurora-engine) contract. 

It is a set of tools for parsing, indexing, preparing migration, and 
migrating data from the [Aurora Engine](https://github.com/aurora-is-near/aurora-engine) contract 
to the new [aurora-eth-connector](https://github.com/aurora-is-near/aurora-eth-connector) contract.

### The set of tools includes

- `parse` - parse for Aurora Engine state snapshot
- `indexer` - indexing NEAR blockchain blocks which include transactions of Aurora Engine contract
- `prepare-migrate-indexed` - prepare data for migration from indexed data
- `migration` - migrate Aurora Engine contract NEP-141 state to `aurora-eth-connector` contract.
- `CLI` - commands and parameters to interact with the application.

# Common migration flow

1. Run migration-tool `indexer`.
2. Run getting snapshot from Aurora contract. It can take more than 2 hours.
3. After snapshot is ready, pause (set to **read-only** mod) Aurora contract and Bridge.
4. Deploy new `aurora-engine` contract with `Splitting NEP-141` functionality.
5. Deploy `aurora-eth-connector`.
6. Run migration-tool `parse` for snapshot file.
7. Run migration-tool `prepare-for-migration` for parsed Aurora state result data (for ex: `migration_state.borsh`).
8. Stop migration-tool `indexer`
9. Run migration-tool `prepare-for-migration` for indexed result data (for ex: `migration_indexed.borsh`).
10. Run migration-tool `combine-indexed-and-state-data` for indexed and state data (for ex: `migration_indexed.borsh` and `migration_state.borsh`).
11. Run migration-tool `migrate` for previously generated `migration_full.borsh`.
12. Unpause Aurora contract and Bridge.

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
it to the resulting file. The collected data is serialized by `borsh`.

```
Parse Aurora Engine contract state snapshot and store result to file serialized with borsh

Usage: aurora-engine-migration-tool parse [OPTIONS] --file <FILE>

Options:
  -f, --file <FILE>    Aurora Engine snapshot json file
  -o, --output <FILE>  Output file with results data serialized with  borsh
  -h, --help           Print help

```

Example:

```
$ aurora-engine-migration-tool parse --file engine-snaphot-2023-05-03-120132.json -o result_file.borsh
```

**IMPORTANT NOTICE**: the data that the parsing operation returns is 
not suitable for migration. Therefore, the command `prepare-for-migration` must be called 
before the migration. This will receive from the Aurora contract 
the current state of the accounts - their balances. And it is 
important to note that the Aurora contract must be on pause and in 
the READ ONLY status. Those. its state does not change. This ensures 
that correct data is received. Therefore, before migration, the 
operation of `prepare-for-migration` is mandatory.


## Indexer

**How is data indexed?** It is possible to index Aurora Engine contract 
data via NEAR RPC - mainnet, mainnet-archival (after 250000 from current 
block), testnet. Only successful blocks and chunks are indexed. All 
NEAR shards are also processed. That guarantees receipt of all 
necessary data. Transactions for the `aurora` contract are parsed. At 
the same time, transactions in which methods are called that only 
apply to `eth-connector` (including those related to the NEP-141 
standard):

- `ft_transfer` - parsed method arguments and `predecessor_id`. Gather only accounts.
- `ft_transfer_call` - parsed method arguments and `predecessor_id`. Gather only accounts.
- `deposit` - parsed only `predecessor_id`. Gather only accounts.
- `withdraw` - parsed only `predecessor_id`. Gather only accounts.
- `finish_deposit` - parsed method arguments and `predecessor_id`. Gather accounts and deposit proof data.
- `storage_deposit` parsed method arguments and `predecessor_id`. Gather only accounts.
- `storage_withdraw` - parsed only `predecessor_id`. Gather only accounts.
- `storage_unregister` - parsed only `predecessor_id`. Gather only accounts.

**IMPORTANT NOTICE**: we need only accounts without balances (balances 
will be received with the command `prepare-migrate-indexed`), and proof 
data. You **MUST** run the command `prepare-migrate-indexed` after 
indexing data.

```
Run indexing NEAR blockchain blocks and chunks for all shards, for specific NEAR network. For Aurora Engine contract.

Usage: aurora-engine-migration-tool indexer [OPTIONS]

Options:
  -H, --history               Indexing missed historical blocks
  -F, --force                 Force get blocks without check current block for historical and specific block indexing
  -s, --stat                  Show short indexed statistic
      --fullstat              Show full indexed statistic
  -b, --block <BLOCK_HEIGHT>  Start indexing from specific block
  -h, --help                  Print help
```


## Prepare data for migration after indexing

Data received after indexing is
not suitable for migration. Therefore, the command `prepare-migrate-indexed` must be called
before the migration. This command receive from the Aurora contract
the current state of the accounts - their balances. And it is
important to note that the Aurora contract must be on pause and in
the READ ONLY status. Those. its state does not change. This ensures
that correct data is received. Therefore, before migration after indexing, the
operation of `prepare-migrate-indexed` is mandatory.

```
Prepare indexed data for migration. Should be invoked befor migration

Usage: aurora-engine-migration-tool prepare-migrate-indexed --file <FILE> --output <FILE>

Options:
  -f, --file <FILE>    File with parsed or indexed data serialized with borsh
  -o, --output <FILE>  Output file with migration results data serialized with borsh
  -h, --help           Print help
```

Example:

```
$ aurora-engine-migration-tool prepare-migrate-indexed --file indexed_data.borsh --output data_for_migration.borsh 
```


## Migration

**IMPORTANT NOTICE**: there is no need to generate 
special data that relates to the NEP-141 `storage_deposit` 
function, since the deposit of the storage occurs 
through the attachment of tokens during the transaction for 
`storage_deposit`. Accordingly, there is no information about this. 
Those. we just need to know the account values for these functions from 
arguments and `predecessor_id`. For `storage_withdraw` - this function
do nothing with account. `storage_unregister` just delete account,
but we still need to know it, because we can store just zero balance.
`storage_unregister` just remove account entity and costs nothing. In 
`aurora-eth-connector` context the deleted entity and account with zero balance
has same sense.

Parameters:
- `--contract` - contract name for migration. Ex: `some-acc.testnet`.
- `--signer` - signer account id for migration. Ex: `some-acc.testnet`.
- `--key` - Account private key for sign migration transactions.
- `--file` - input file that contain borsh serialized data for the migration.

Example:

```
$ aurora-engine-migration-tool migrate --contract ${ACCOUNT_ID} --signer ${ACCOUNT_ID} --key ${ACCOUNT_KEY} --file contract_state.borsh
```

# Features flags

This set of tools can be used for both NEAR `mainten`, 
`testnet` and `localnet`. It is important to specify the 
appropriate flag explicitly.

Available options:

- `mainnet` - NEAR mainnet.
- `mainnet-archival` - NEAR mainnet-archival (after 250000 blocks from current RPC should call `archival` data).
- `testnet` - NEAR testnet.
- `localnet` - manually started NEAR localnet.
- `log` - show log data in application output.


## Useful commands

- `make check` - run cargo `fmt` and `clippy` for all features (default command).
- `make build-mainnet-release` - build release version of application for NEAR mainnet. 
- `make build-testnet-release` - build release version of application for NEAR testnet.
- `migrate-testnet` - build testnet release and run migration with parameters from environment:
    - `--contract ${ACCOUNT_ID}` - contract name for migration. Ex: `some-acc.testnet`.
    - `--signer ${ACCOUNT_ID}` - signer account id for migration. Ex: `some-acc.testnet`.
    - `--key ${ACCOUNT_KEY}` - Account private key for sign migration transactions. 
    - `--file contract_state.borsh` - input data for migration.
- `make index-fullstat` - build indexer and run full-stat command for indexer.
- `index-stat`- build indexer and run short statistics for indexer.
- `index-history` - build indexer and run indexing historical data.
- `make prepare-migrate-indexed` - build indexer and run data preparation with params:
  - `-f data.borsh` - input prepared data (for example from indexer)
  - `-o for-migtation.borsh` - output file, that can be used to run migration.

## Test via localnet

To test very basic flow, it's useful to run test script for `NEAR localnet`.
Please make sure that `python` and `pip` is installed.

#### How to use test script

```
cd scripts
./test_flow.sh
```

### LICENSE: [CC0 1.0 Universal](LICENSE)
