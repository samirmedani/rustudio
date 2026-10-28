#!/bin/sh
# Verification locale avant une pull request : tests sur la fausse GoXLR et recherche de donnees personnelles.
set -e
cd "$(dirname "$0")/.."
cargo test --locked
python3 scripts/leakscan.py
