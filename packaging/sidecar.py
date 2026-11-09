#!/usr/bin/env python3
"""Build the engine job binary in release and place it where the Tauri
bundler expects a sidecar: crates/app/binaries/model-lab-engine-job-<triple>.

    python packaging/sidecar.py          build and copy
    python packaging/sidecar.py --check  verify the copy exists and matches
                                         the release build (exit 2 otherwise)

Standard library only.
"""
from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BINARIES = ROOT / "crates" / "app" / "binaries"
NAME = "model-lab-engine-job"


def host_triple() -> str:
    out = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise SystemExit("rustc -vV did not report a host triple")


def exe(name: str, triple: str) -> str:
    return name + (".exe" if "windows" in triple else "")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def target_dir() -> Path:
    return Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))


def main(argv: list[str]) -> int:
    triple = host_triple()
    built = target_dir() / "release" / exe(NAME, triple)
    copy = BINARIES / exe(f"{NAME}-{triple}", triple)
    if "--check" in argv:
        if not copy.is_file():
            print(f"sidecar missing: {copy.relative_to(ROOT)}", file=sys.stderr)
            return 2
        if not built.is_file():
            print(f"release build missing: {built}", file=sys.stderr)
            return 2
        if sha256(copy) != sha256(built):
            print(f"sidecar is stale: {copy.relative_to(ROOT)} differs from {built}", file=sys.stderr)
            return 2
        print(f"sidecar ok: {copy.relative_to(ROOT)}")
        return 0
    env = dict(os.environ)
    if "--remap-path-prefix" not in env.get("RUSTFLAGS", ""):
        # No build-machine paths in the shipped sidecar (panic locations).
        registry = Path.home() / ".cargo" / "registry"
        env["RUSTFLAGS"] = " ".join(
            filter(None, [env.get("RUSTFLAGS", ""), f"--remap-path-prefix={registry}=cargo-registry", f"--remap-path-prefix={ROOT}=model-lab"])
        )
    subprocess.run(
        ["cargo", "build", "-p", "model-lab-engine", "--bin", NAME, "--release", "--locked"],
        check=True,
        cwd=ROOT,
        env=env,
    )
    BINARIES.mkdir(parents=True, exist_ok=True)
    shutil.copy2(built, copy)
    print(f"sidecar: {copy.relative_to(ROOT)} ({sha256(copy)[:12]})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
