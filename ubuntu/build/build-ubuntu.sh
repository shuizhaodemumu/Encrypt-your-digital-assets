#!/bin/sh
# Run from an Ubuntu x86_64 development machine with the documented build dependencies.
set -eu
cd "$(dirname -- "$0")/.."
mkdir -p work
test_state=$(mktemp -d "$PWD/work/unit-state-XXXXXX")
trap 'rm -rf -- "$test_state"' EXIT HUP INT TERM
cargo fmt --check
ENCRYPT_ASSETS_DATA_DIR="$test_state" cargo test --locked --features watchdog-test-mode -- --test-threads=1
cargo build --locked --release
python3 build/bundle.py target/release/encrypt-assets-ubuntu --suffix release
cd outputs
sha256sum Encrypt-assets-Ubuntu-0.1.3-release-x86_64.run encrypt-assets-ubuntu_0.1.3_amd64.deb > SHA256SUMS.txt
