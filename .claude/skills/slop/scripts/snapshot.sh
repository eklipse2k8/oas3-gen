#!/usr/bin/env bash
# Writes the generator's client-mod and server-mod output for fixtures/petstore.json
# into the given directory, replacing anything already there.
set -euo pipefail

out="${1:?usage: snapshot.sh <output-dir>}"
root="$(git rev-parse --show-toplevel)"

rm -rf "$out"
for mode in client-mod server-mod; do
  cargo run -q --manifest-path "$root/Cargo.toml" -- generate "$mode" \
    -i "$root/fixtures/petstore.json" -o "$out/$mode" \
    --workspace --enable-builders --all-schemas --all-headers > /dev/null
done
