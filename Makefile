# Same commands as the AGENTS.md local-gate fence. First clone: make check
.PHONY: check
check:
	cargo fmt --check
	cargo clippy --locked --workspace --all-targets -- -D warnings
	cargo nextest run --locked --workspace
	cargo test --locked --workspace --doc
	bash factory/scripts/deny-check.sh
	bash factory/scripts/assert-stealth.sh craftbag/craftbag
	bash factory/scripts/write-ledger.sh --self-test
