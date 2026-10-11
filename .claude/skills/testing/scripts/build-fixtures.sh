#!/usr/bin/env bash
# Regenerates fixtures/petstore-client and fixtures/petstore-server from fixtures/petstore.json,
# then applies clippy fixes and nightly rustfmt so the output matches the committed fixtures.
# Prints the resulting fixture diff stat.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

for side in client server; do
  cargo run -q -- generate "$side-mod" -i fixtures/petstore.json -o "fixtures/petstore-$side" \
    --workspace --enable-builders --all-schemas --all-headers > /dev/null
done

log="$(mktemp)"
trap 'rm -f "$log"' EXIT
cargo clippy -q --fix --allow-dirty --all --all-targets -- -W clippy::pedantic 2> "$log" \
  || { cat "$log" >&2; exit 1; }
cargo +nightly fmt --all

git diff --stat -- fixtures/
