#!/usr/bin/env bash

set -e

FILE="${PORT:-/tmp/trace.psf}"
IPADDR="${IPADDR:-192.0.2.80}"
PORT="${PORT:-8888}"

echo "Writing trace data to $FILE"

echo -ne '\x01\x01\x00\x00\x00\x00\xFD\xFF' | nc -w 60 ${IPADDR} ${PORT} > $FILE

exit 0
