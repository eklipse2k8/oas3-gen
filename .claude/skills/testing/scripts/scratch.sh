#!/usr/bin/env bash
# Creates a throwaway crate at /tmp/oas3-gen-scratch/<name> for proving behavior outside the repo.
# The crate's library is the code generated from <spec> (default fixtures/petstore.json) in
# client-mod or server-mod mode (default client-mod). It builds against the local
# oas3-gen-support, and its tests/ can use oas3-gen, rstest, proptest, and tokio.
# Every scratch crate shares /tmp/oas3-gen-scratch/target, so only the first build is cold.
set -euo pipefail

name="${1:?usage: scratch.sh <name> [client-mod|server-mod] [spec]}"
mode="${2:-client-mod}"
root="$(git rev-parse --show-toplevel)"
spec="$(realpath "${3:-$root/fixtures/petstore.json}")"
base=/tmp/oas3-gen-scratch
dir="$base/$name"

rm -rf "$dir"
cargo run -q --manifest-path "$root/Cargo.toml" -- generate "$mode" -i "$spec" -o "$dir" \
  --workspace --enable-builders --all-schemas --all-headers > /dev/null

{
  printf '\n[workspace]\n\n[patch.crates-io]\n'
  printf 'oas3-gen-support = { path = "%s/crates/oas3-gen-support" }\n' "$root"
  printf '\n[dev-dependencies]\n'
  printf 'oas3-gen = { path = "%s/crates/oas3-gen" }\n' "$root"
  sed -n '/^\[workspace.dependencies\]/,/^\[/p' "$root/Cargo.toml" | grep -E '^(anyhow|proptest|rstest|serde_json|tokio) '
} >> "$dir/Cargo.toml"

cp "$root/Cargo.lock" "$dir/Cargo.lock"
mkdir -p "$dir/.cargo" "$dir/tests"
printf '[build]\ntarget-dir = "%s/target"\n' "$base" > "$dir/.cargo/config.toml"

echo "$dir"
