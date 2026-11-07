#!/bin/sh
set -eu
cd "$(dirname "$0")"
uv sync --locked --python 3.14
cargo test --locked
cargo build --release --locked
uv run --offline python packaging/build_macos.py
