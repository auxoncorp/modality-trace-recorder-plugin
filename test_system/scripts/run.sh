#!/bin/bash

set -euo pipefail

(
    cd build
    make emulate
)

exit 0
