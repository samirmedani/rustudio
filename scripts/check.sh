#!/bin/sh
# Verification locale avant une pull request : tests sur la fausse GoXLR et recherche de donnees personnelles.
set -e
cd "$(dirname "$0")/.."
cargo test --locked
cargo test --locked -p rustudio-goxlr --example usbstats
node scripts/diagnostic-ui-test.mjs
python3 scripts/leakscan.py
