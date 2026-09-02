#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
TARGET_TRIPLE="aarch64-apple-darwin"
RELEASE_DIR="${ROOT_DIR}/target/${TARGET_TRIPLE}/release"

cd "${ROOT_DIR}"

echo "Checking formatting..."
cargo fmt --check

echo "Running tests..."
cargo test

echo "Running Clippy..."
cargo clippy

echo "Building the Apple Silicon release binary..."
cargo build --release --target "${TARGET_TRIPLE}"

echo "Packaging the macOS application..."
cargo packager --release

DMG_PATH="$(find "${RELEASE_DIR}" -type f -name '*.dmg' -print -quit)"
if [[ -z "${DMG_PATH}" ]]; then
    echo "Packaging completed without producing a DMG in ${RELEASE_DIR}." >&2
    exit 1
fi

printf 'DMG: %s\n' "${DMG_PATH}"
