#!/bin/bash

set -euo pipefail

export RUST_LOG=warn
#export RUST_LOG=modality_trace_recorder=debug

modality-reflector import --config config/reflector_config_rtt.toml trace-recorder /tmp/rtt_log.bin

modality workspace sync-indices

exit 0
