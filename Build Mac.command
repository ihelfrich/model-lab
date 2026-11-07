#!/bin/sh
set -eu
cd "$(dirname "$0")"
uv sync --locked --python 3.14
cargo test --locked --features acceptance
# The shipped binary carries no verification harness; build it with
# `--features acceptance` into a separate target dir when running verify_*.py.
# Strip build-machine paths (panic locations) from the shipped binary.
RUSTFLAGS="--remap-path-prefix=$HOME/.cargo/registry=cargo-registry --remap-path-prefix=$PWD=model-lab" cargo build --release --locked
uv run --offline python packaging/build_macos.py
