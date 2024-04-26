#!/usr/bin/env bash

set -euo pipefail

SEGGER_PATH="${SEGGER_PATH:-/opt/JLink}"

datetime=$(date +'%Y-%m-%dT%H-%M-%SZ')

# Issue a reset so the time between start-of-tracing on-target and host-side reading is minimal
"${SEGGER_PATH}/JLinkExe" \
    -CommandFile "$(pwd)/reset_board.jlink"

# NOTE: if you know the RTT control block address (e.g. using binutils to lookup the symbol)
#   arm-none-eabi-readelf -s build/m4-refboard-streaming-rtt | grep _SEGGER_RTT
# you can add this option to skip searching memory
# -RTTAddress 0x<hex-address>

"${SEGGER_PATH}/JLinkRTTLogger" \
    -Device STM32F407VE \
    -If SWD \
    -USB 932000284 \
    -Speed 4000 \
    -RTTChannel 1 \
    "$(pwd)/trace_${datetime}.psf"

exit 0
