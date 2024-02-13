#!/bin/bash

set -euo pipefail

RUST_LOG=error modality-reflector run --config reflector-config.toml --collector trace-recorder-tcp

exit 0
