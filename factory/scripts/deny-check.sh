#!/usr/bin/env bash
# License and advisory policy for the crate workspace and cargo-fuzz workspace.
# fuzz/ is exclude= from the root workspace, so a root-only deny check never
# sees libfuzzer-sys ((MIT OR Apache-2.0) AND NCSA).
set -euo pipefail

echo "PLAN: cargo deny (workspace + fuzz)"
echo "DO: cargo deny check"
cargo deny check
echo "DO: cargo deny --manifest-path fuzz/Cargo.toml check"
cargo deny --manifest-path fuzz/Cargo.toml check
echo "DONE: ok=true"
