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
# REQUIREMENTS
# - ACCOUNT_ENGINE_ID env variable
# - ACCOUNT_ETH_CONNECTOR_ID env variable
# - ACCOUNT_ETH_CONNECTOR_PRIV_KEY env variable

#==================================
# Init variables
export NEARCORE_HOME="/tmp/localnet"

AURORA_LAST_VERSION="3.3.1"
USER_BASE_BIN=$(python3 -m site --user-base)/bin
ENGINE_LAST_WASM_URL="https://github.com/aurora-is-near/aurora-engine/releases/download/$AURORA_LAST_VERSION/aurora-mainnet.wasm"
ENGINE_WASM="/tmp/aurora-mainnet.wasm"
NODE_KEY_PATH=$NEARCORE_HOME/node0/validator_key.json
AURORA_KEY_PATH=$NEARCORE_HOME/node0/aurora_key.json
ETH_CONNECTOR_KEY_PATH=$NEARCORE_HOME/node0/eth_connector_key.json
ENGINE_ACCOUNT=aurora.node0
ETH_CONNECTOR_ACCOUNT=eth-connector.node0
ETH_CONNECTOR_WASM=/tmp/aurora-eth-connector/bin/aurora-eth-connector-mainnet.wasm

export PATH="$PATH:$USER_BASE_BIN:$HOME/.cargo/bin"
#==================================
# Util functions
install_nearup() {
  pip3 list | grep nearup > /dev/null || pip3 install --user nearup
}

start_node() {
  cmd="nearup run localnet --home $NEARCORE_HOME"

  if [[ $(uname -m) == "arm64" ]]; then # Check for local execution
    cmd="$cmd --binary-path $HOME/.nearup/near/localnet --num-nodes 1"
  fi
  # $cmd > /dev/null 2>&1
  $cmd
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
  if [[ $1 != $2 ]]; then
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

download_aurora_contact() {
  curl -sL $ENGINE_LAST_WASM_URL -o $ENGINE_WASM || error_exit
}

get_eth_connector_and_build_for_migration() {
  rm -rf /tmp/aurora-eth-connector
  git clone https://github.com/aurora-is-near/aurora-eth-connector.git /tmp/aurora-eth-connector  > /dev/null 2>&1
  cd /tmp/aurora-eth-connector || error_exit
  cargo make --profile=mainnet build-migration > /dev/null 2>&1
}

#==================================
# Main

rm -rf /tmp/localnet

echo "Install nearup"
install_nearup

echo "Start NEAR node"
start_node
sleep 1

#echo "Download Aurora contract"
#download_aurora_contact

export NEAR_KEY_PATH=$NODE_KEY_PATH
echo "Create account for Aurora"
aurora-cli create-account --account $ENGINE_ACCOUNT --balance 1000 > $AURORA_KEY_PATH || error_exit
sleep 1

#echo "View info of created Aurora account"
#balance=$(aurora-cli view-account $ENGINE_ACCOUNT  | jq '.amount') || error_exit
#sleep 1
## assert_eq $balance "1000000000000000000000000000"
#echo $balance
#
#export NEAR_KEY_PATH=$AURORA_KEY_PATH
#echo "Deploy Aurora contract"
#aurora-cli deploy-aurora $ENGINE_WASM || error_exit
#sleep 4
#
#echo "Init Aurora"
#aurora-cli --engine $ENGINE_ACCOUNT init \
#  --chain-id 1313161556 \
#  --owner-id $ENGINE_ACCOUNT \
#  --bridge-prover-id "prover" \
#  --upgrade-delay-blocks 1 \
#  --custodian-address 0x1B16948F011686AE64BB2Ba0477aeFA2Ea97084D \
#  --ft-metadata-path engine_ft_metadata.json || error_exit
#sleep 2
#
#echo "Get Aurora contract version"
#version=$(aurora-cli --engine $ENGINE_ACCOUNT get-version || error_exit)
#sleep 1
#assert_eq "$version" $AURORA_LAST_VERSION
#echo "$version"
#
#echo "Get and build Aurora Eth-Connector for migration"
#get_eth_connector_and_build_for_migration

export NEAR_KEY_PATH=$NODE_KEY_PATH
echo "Create account for Eth-Connector"
aurora-cli create-account --account $ETH_CONNECTOR_ACCOUNT --balance 1000 > $ETH_CONNECTOR_KEY_PATH || error_exit
sleep 1

echo "View info of created Eth-Connector account"
balance=$(aurora-cli view-account $ETH_CONNECTOR_ACCOUNT | jq '.amount') || error_exit
sleep 1
# assert_eq "$balance" "1000000000000000000000000000"
echo $balance

export NEAR_KEY_PATH=$ETH_CONNECTOR_KEY_PATH
echo "Deploy Eth-Connector contract for migration"
near deploy --keyPath $ETH_CONNECTOR_KEY_PATH --network_id localnet --nodeUrl  http://127.0.0.1:3030 -v $ETH_CONNECTOR_ACCOUNT $ETH_CONNECTOR_WASM new "$(cat init_eth_connector.json)" || error_exit
sleep 4

echo "Call Eth-Connector deposit"
# TODO: add  --base64
near call --keyPath --account_id $ETH_CONNECTOR_KEY_PATH $ETH_CONNECTOR_KEY_PATH --network_id localnet --nodeUrl  http://127.0.0.1:3030 -v $ETH_CONNECTOR_ACCOUNT "$(cat deposit_proof_data.json)" || error_exit

echo "Finish: stop NEAR node and clean up"
#finish