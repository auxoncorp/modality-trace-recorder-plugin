#!/bin/bash

set -euo pipefail

(
    cd build
    make emulate
)

sleep 1

modality internal sync-indices

exit 0
