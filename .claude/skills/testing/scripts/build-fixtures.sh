#!/usr/bin/env bash
# Regenerates fixtures/petstore-client and fixtures/petstore-server from fixtures/petstore.json,
# marks both `publish = false`, rewrites each generated dependency to inherit from the root
# `[workspace.dependencies]`, then applies clippy fixes and nightly rustfmt so the output matches
# the committed fixtures.
# Prints the resulting fixture diff stat.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

for side in client server; do
  manifest="fixtures/petstore-$side/Cargo.toml"
  cargo run -q -- generate "$side-mod" -i fixtures/petstore.json -o "fixtures/petstore-$side" \
    --workspace --enable-builders --all-schemas --all-headers > /dev/null
  perl -pi -e '$_ .= "publish = false\n" if /^name = /' "$manifest"
  perl -pi -e '
    if (/^\[/) { $deps = /^\[dependencies\]$/ }
    elsif ($deps) {
      s/^([\w-]+) = "[^"]*"$/$1.workspace = true/
        || s/^([\w-]+) = \{ version = "[^"]*"/$1 = { workspace = true/;
    }
  ' "$manifest"
done

log="$(mktemp)"
trap 'rm -f "$log"' EXIT
cargo clippy -q --fix --allow-dirty --all --all-targets -- -W clippy::pedantic 2> "$log" \
  || { cat "$log" >&2; exit 1; }
cargo +nightly fmt --all

git diff --stat -- fixtures/
