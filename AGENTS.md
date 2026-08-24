# Agents

Local gate before every commit:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
cargo deny check
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
