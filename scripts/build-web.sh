#!/usr/bin/env bash
# Builds the web frontend into crates/nesrs-web/www/pkg.
#
# Requires the wasm32 target and a wasm-bindgen CLI matching the crate version:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.130 --locked
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p nesrs-web --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript \
  --out-dir crates/nesrs-web/www/pkg \
  target/wasm32-unknown-unknown/release/nesrs_web.wasm

echo
echo "Built crates/nesrs-web/www. Serve it with any static file server, e.g.:"
echo "  python3 -m http.server -d crates/nesrs-web/www 8080"
