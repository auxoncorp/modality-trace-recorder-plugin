#!/bin/bash

set -euo pipefail

export RUST_LOG=warn
#export RUST_LOG=modality_trace_recorder=debug

modality-reflector run --config config/reflector_config_tcp.toml --collector trace-recorder-tcp

modality workspace sync-indices

exit 0
