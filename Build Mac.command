#!/bin/sh
set -eu
cd "$(dirname "$0")"
uv sync --locked --python 3.14
cargo test --workspace --locked --features tree-ensemble-lab/acceptance
# The shipped binary carries no verification harness; build it with
# `--features acceptance` into a separate target dir when running verify_*.py.
# Strip build-machine paths (panic locations) from the shipped binary.
RUSTFLAGS="--remap-path-prefix=$HOME/.cargo/registry=cargo-registry --remap-path-prefix=$PWD=model-lab" cargo build --release --locked
uv run --offline python packaging/build_macos.py
# Model Lab (the Tauri shell): engine-job sidecar, front end, bundle.
# The Tauri CLI runs from crates/app; its before-build command runs in ui/.
uv run --no-project --offline python packaging/sidecar.py
uv run --no-project --offline python packaging/sidecar.py --check
( cd crates/app/ui && npm ci )
( cd crates/app && RUSTFLAGS="--remap-path-prefix=$HOME/.cargo/registry=cargo-registry --remap-path-prefix=$PWD/../..=model-lab" ui/node_modules/.bin/tauri build --ci )
echo "Model Lab bundle: target/release/bundle/macos/Model Lab.app"
