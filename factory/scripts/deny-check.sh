#!/usr/bin/env bash
# License and advisory policy for the crate workspace and cargo-fuzz workspace.
# fuzz/ is exclude= from the root workspace, so a root-only deny check never
# sees libfuzzer-sys ((MIT OR Apache-2.0) AND NCSA).
# Root Cargo.toml is a package plus workspace, not a virtual manifest, so
# cargo-deny does not assume --workspace. Without it the graph is the library
# only and craftbag-cli / craftbag-mcp runtime deps (clap, serde_json) are
# never checked.
set -euo pipefail

echo "PLAN: cargo deny (workspace + fuzz, locked)"
echo "DO: cargo deny --workspace --locked check"
cargo deny --workspace --locked check
echo "DO: cargo deny --manifest-path fuzz/Cargo.toml --locked check"
cargo deny --manifest-path fuzz/Cargo.toml --locked check
echo "DONE: ok=true"
