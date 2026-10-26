#!/usr/bin/env bash
set -euo pipefail

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  sh /tmp/rustup-init.sh -y --profile minimal
fi
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
rustup target add wasm32-unknown-unknown
if ! command -v wasm-pack >/dev/null 2>&1; then
  cargo install wasm-pack --locked
fi
wasm-pack build crates/nes-wasm --target web --release --out-dir ../../web/pkg
mkdir -p _site/nes-rs/pkg
cp web/index.html web/app.js web/style.css web/tv.svg web/controller.svg _site/nes-rs/
cp -R web/pkg/. _site/nes-rs/pkg/
touch _site/nes-rs/.nojekyll
