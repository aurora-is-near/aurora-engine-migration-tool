#!/bin/bash

#==================================
# DESCRIPTION
# This script reproduce complex flow:
# 1. Deploy and init aurora-engin new contract
# 2. Deposit funds to aurora-engin
# 3. Verify balance for deposited account_id
# 4. Deploy aurora-eth-connector new contract
# 5. Call near get contract state
# 6. Invoke migration process
# 7. Manually check migration results

#==================================
# Init variables
export NEARCORE_HOME="/tmp/localnet"

AURORA_LAST_VERSION="3.6.3"
USER_BASE_BIN=$(python3 -m site --user-base)/bin
ENGINE_LAST_WASM_URL="https://github.com/aurora-is-near/aurora-engine/releases/download/$AURORA_LAST_VERSION/aurora-mainnet.wasm"
ENGINE_WASM="/tmp/aurora-contract/target/wasm32-unknown-unknown/release/aurora_engine.wasm"
NODE_KEY_PATH=$NEARCORE_HOME/node0/validator_key.json
AURORA_KEY_PATH=$NEARCORE_HOME/node0/aurora_key.json
ETH_CONNECTOR_KEY_PATH=$NEARCORE_HOME/node0/eth_connector_key.json
ENGINE_ACCOUNT=aurora.node0
ETH_CONNECTOR_ACCOUNT=eth-connector.node0
ETH_CONNECTOR_WASM=/tmp/aurora-eth-connector/bin/aurora-eth-connector-test.wasm
PROOF="AQAAAAAAAAD9AAAA+PuUCW3pwriluMIs7jKJsQH2lg1o5R74QqDRQkOcJ44l2tmlB2bxU9Dj0te/K9FvwngcS9SUsrFanaAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAALigAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAHgeAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABF0ZXN0X2FjY291bnQubmVhcgAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAA"
GAS=300000000000000
MIGRATION_TOOL=../target/release/aurora-engine-migration-tool

export PATH="$PATH:$USER_BASE_BIN:$HOME/.cargo/bin"
#==================================
# Util functions
install_nearup() {
  pip3 list | grep nearup > /dev/null || pip3 install --user nearup
}

start_node() {
  rm -rf $NEARCORE_HOME
  cmd="nearup run localnet --home $NEARCORE_HOME"

  if [[ $(uname -m) == "arm64" ]]; then # Check for local execution
    cmd="$cmd --binary-path $HOME/.nearup/near/localnet --num-nodes 1"
  fi
  $cmd > /dev/null 2>&1
}

stop_node() {
  nearup stop > /dev/null 2>&1
}

finish() {
  echo "Stop NEAR node"
  stop_node
  echo "Cleanup"
  rm -rf $NEARCORE_HOME

  if [[ -z "$1" ]]; then
    exit 0
  else
    exit "$1"
  fi
}

error_exit() {
  finish 1
}

assert_eq() {
  a=$(echo "$1" | tr -d '"')
  b=$(echo "$2" | tr -d '"')

  if [[ "$a" != "$b" ]]; then
    echo "Unexpected result, should be $1 but actual is $2"
    finish 1
  fi
}

check_env_var() {
  if [ -z "$(eval echo \$$1)" ]; then
      echo "$1 environment variable doesn't exist."
      error_exit
  fi
}

download_aurora_contract() {
  curl -sL $ENGINE_LAST_WASM_URL -o $ENGINE_WASM || error_exit
}

get_aurora_and_build() {
  curr_dir=$(pwd)
  rm -rf /tmp/aurora-contract
  git clone https://github.com/aurora-is-near/aurora-engine.git /tmp/aurora-contract > /dev/null 2>&1
  cd /tmp/aurora-contract || error_exit
  # cargo make --profile=mainnet build-test > /dev/null 2>&1 || error_exit
  RUSTFLAGS="-C link-arg=-s" cargo build --target wasm32-unknown-unknown --release --no-default-features --features=mainnet,integration-test -p aurora-engine -Z avoid-dev-deps > /dev/null 2>&1 || error_exit
  cd $curr_dir || error_exit
}

build_migration_tool() {
  cd ..
  cargo build --features localnet --release > /dev/null 2>&1 || error_exit
  cd scripts || error_exit
}

get_eth_connector_and_build_for_migration() {
  curr_dir=$(pwd)
  rm -rf /tmp/aurora-eth-connector
  git clone https://github.com/aurora-is-near/aurora-eth-connector.git /tmp/aurora-eth-connector  > /dev/null 2>&1
  cd /tmp/aurora-eth-connector || error_exit
  cargo make --profile=mainnet build-test > /dev/null 2>&1 || error_exit
  cd $curr_dir || error_exit
}

get_aurora_contract_state() {
  http post http://127.0.0.1:3030 jsonrpc=2.0 id=dontcare method=query \
    params:='{
      "request_type": "view_state",
      "finality": "final",
      "account_id": "'"$ENGINE_ACCOUNT"'",
      "prefix_base64": ""
    }' > res_state.json
}

#==================================
# Main

echo "Install nearup"
install_nearup

echo "Start NEAR node"
start_node
sleep 2

echo "Get and build Aurora contract"
get_aurora_and_build

export NEAR_KEY_PATH=$NODE_KEY_PATH
echo "Create account for Aurora"
aurora-cli create-account --account $ENGINE_ACCOUNT --balance 1000 > $AURORA_KEY_PATH || error_exit
sleep 2

echo "View info of created Aurora account"
balance=$(aurora-cli view-account $ENGINE_ACCOUNT  | jq '.amount') || error_exit
sleep 1
assert_eq "$balance" "1000000000000000000000000000"
echo $balance

export NEAR_KEY_PATH=$AURORA_KEY_PATH
echo "Deploy Aurora contract"
aurora-cli deploy-aurora $ENGINE_WASM || error_exit
sleep 4

echo "Init Aurora"
aurora-cli --engine $ENGINE_ACCOUNT init \
  --chain-id 1313161556 \
  --owner-id $ENGINE_ACCOUNT \
  --bridge-prover-id "aurora.node0" \
  --upgrade-delay-blocks 1 \
  --custodian-address 0x096DE9C2B8A5B8c22cEe3289B101f6960d68E51E \
  --ft-metadata-path engine_ft_metadata.json || error_exit
sleep 2

echo "Get Aurora contract version"
version=$(aurora-cli --engine $ENGINE_ACCOUNT get-version || error_exit)
sleep 1
assert_eq "$version" $AURORA_LAST_VERSION
echo "$version"

echo "Call Aurora deposit"
near call $ENGINE_ACCOUNT deposit $PROOF --base64 --accountId $ENGINE_ACCOUNT --keyPath $AURORA_KEY_PATH --network_id localnet --nodeUrl  http://127.0.0.1:3030 --gas $GAS > /dev/null || error_exit
sleep 1

echo "Get deposited balance"
near view $ENGINE_ACCOUNT ft_balance_of  '{"account_id": "test_account.near"}' --keyPath $AURORA_KEY_PATH --network_id localnet --nodeUrl  http://127.0.0.1:3030 || error_exit

echo "Get Aurora contract state"
get_aurora_contract_state

echo "Get and build Aurora Eth-Connector for migration"
get_eth_connector_and_build_for_migration

export NEAR_KEY_PATH=$NODE_KEY_PATH
echo "Create account for Eth-Connector"
aurora-cli create-account --account $ETH_CONNECTOR_ACCOUNT --balance 1000 > $ETH_CONNECTOR_KEY_PATH || error_exit
sleep 2

echo "View info of created Eth-Connector account"
balance=$(aurora-cli view-account $ETH_CONNECTOR_ACCOUNT | jq '.amount') || error_exit
sleep 1
assert_eq $balance "1000000000000000000000000000"
echo $balance

export NEAR_KEY_PATH=$ETH_CONNECTOR_KEY_PATH
echo "Deploy Eth-Connector contract for migration"
near deploy --keyPath $ETH_CONNECTOR_KEY_PATH --network_id localnet --nodeUrl  http://127.0.0.1:3030 $ETH_CONNECTOR_ACCOUNT $ETH_CONNECTOR_WASM new "$(cat init_eth_connector.json)" > /dev/null || error_exit
sleep 4

echo "Build migration tool"
build_migration_tool

echo "Parse Aurora contract state"
$MIGRATION_TOOL parse -f res_state.json -o res_state.borsh

echo "Migrate data to Eth-Connector"
privkey=$(cat $ETH_CONNECTOR_KEY_PATH | jq '.private_key' | tr -d '"')
echo "$privkey"
$MIGRATION_TOOL migrate --file res_state.borsh --signer "$ETH_CONNECTOR_ACCOUNT" --contract "$ETH_CONNECTOR_ACCOUNT"  --key "$privkey"

sleep 10
echo "Check migration"
$MIGRATION_TOOL check-migration --file res_state.borsh --signer "$ETH_CONNECTOR_ACCOUNT" --contract "$ETH_CONNECTOR_ACCOUNT" --key "$privkey"

echo "Get migrated balance"
near view $ETH_CONNECTOR_ACCOUNT ft_balance_of  '{"account_id": "test_account.near"}' --keyPath $ETH_CONNECTOR_KEY_PATH --network_id localnet --nodeUrl  http://127.0.0.1:3030 || error_exit

echo "Finish: stop NEAR node and clean up"
finish
