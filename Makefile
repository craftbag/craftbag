# Same commands as the AGENTS.md local-gate fence. First clone: make check
# Hosted rust jobs set RUSTFLAGS=-D warnings (workflow env). Clippy
# `-D warnings` is the clippy lint group only; nextest and doctest
# need the rustc flag or a clone can pass while CI fails.
.PHONY: check
check:
	cargo fmt --check
	RUSTFLAGS="-D warnings" cargo clippy --locked --workspace --all-targets -- -D warnings
	RUSTFLAGS="-D warnings" cargo nextest run --locked --workspace
	RUSTFLAGS="-D warnings" cargo test --locked --workspace --doc
	bash factory/scripts/deny-check.sh
	bash factory/scripts/write-ledger.sh --self-test
