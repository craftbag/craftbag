# Agents

> **Human contributors:** This file is for AI coding assistants.
> You can safely ignore it. See README.md and CONTRIBUTING.md instead.

MSRV 1.85 (`rust-toolchain.toml`). No let-chains.

`rust-toolchain.toml` installs rustfmt and clippy. The local gate also
needs cargo-nextest, cargo-deny, and `gh`.
Use prebuilt binaries. Do not `cargo install` those two on rustc 1.85
(they need a newer compiler than this crate's MSRV).

- nextest: https://nexte.st/docs/installation/pre-built-binaries/
- cargo-deny: https://embarkstudios.github.io/cargo-deny/cli/index.html#install-from-binaries
- gh: https://cli.github.com/

Hosted CI also runs fuzz-smoke, gitleaks, actionlint, and zizmor.

Local gate before every commit (`make check`):

```bash
cargo fmt --check
RUSTFLAGS="-D warnings" cargo clippy --locked --workspace --all-targets -- -D warnings
RUSTFLAGS="-D warnings" cargo nextest run --locked --workspace
RUSTFLAGS="-D warnings" cargo test --locked --workspace --doc
bash factory/scripts/deny-check.sh
bash factory/scripts/write-ledger.sh --self-test
```

Every commit needs `git commit -s`. Sign-off email is `git config user.email`.

## Cwd

Refuse to treat findbug or durable `~/bline` as this repo. `git remote` must be `craftbag/craftbag`.

## Factory

Parent session is the outer loop. Read `factory/WORK_SOURCES.md` and run `factory/scripts/next-job.sh`. Do not ask whether to continue. One ready PR at a time. Children return only the block in `factory/CHILD_REPORT.md`.

`.grok/workflows/factory-cycle.rhai` is one cycle, not the factory.

## Public

README, GitHub description, and topics name the product. See `factory/CONSTITUTION.md`.

## Methodology

Constitution + `/design` + executable tests + `/greenfield-project` + `/execute-plan`. Spec Kit is ritual only.
