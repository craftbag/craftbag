# Agents

MSRV 1.85 (`rust-toolchain.toml`). No let-chains.

`rust-toolchain.toml` installs rustfmt and clippy. The local gate also
needs cargo-nextest, cargo-deny, and `gh` (stealth runs `gh repo view`).
Use prebuilt binaries. Do not `cargo install` those two on rustc 1.85
(they need a newer compiler than this crate's MSRV).

- nextest: https://nexte.st/docs/installation/pre-built-binaries/
- cargo-deny: https://github.com/EmbarkStudios/cargo-deny#install
- gh: https://cli.github.com/

Hosted CI also runs fuzz-smoke, gitleaks, actionlint, and zizmor.

Local gate before every commit:

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
bash factory/scripts/deny-check.sh
bash factory/scripts/assert-stealth.sh craftbag/craftbag
bash factory/scripts/write-ledger.sh --self-test
```

Every commit needs `git commit -s`. Sign-off email is `git config user.email`.

## Cwd

Refuse to treat findbug or durable `~/bline` as this repo. `git remote` must be `craftbag/craftbag`.

## Factory

Parent session is the outer loop. Read `factory/WORK_SOURCES.md` and run `factory/scripts/next-job.sh`. Do not ask whether to continue. One ready PR at a time. Children return only the block in `factory/CHILD_REPORT.md`.

`.grok/workflows/factory-cycle.rhai` is one cycle, not the factory.

## Stealth

Public for hosted Actions. Empty description, no topics, no FUNDING, README stays `# craftbag` / `Not ready.` Do not run `/oss-announce`. See `factory/CONSTITUTION.md`.

## Methodology

Constitution + `/design` + executable tests + `/greenfield-project` + `/execute-plan`. Spec Kit is ritual only.
