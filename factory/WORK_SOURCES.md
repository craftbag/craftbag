# Work sources

`next_work_source` in `STATE.json` is authoritative. `phase` is a coarse copy after a successful cycle.

Priority (first match wins):

1. `human_gate` set: stop. Print the message. Do not invent work.
2. `ci`: red or missing main run after squash. Dispatch `gh workflow run CI --ref main` until the merge App exists.
3. `issues`: accepted `ready` issues that are not `needs-triage`.
4. `contract` / `implement`: next slice from `pr_plan_cursor` (starts at `pr-2`).
5. `prove`: `craftbag why` on a real tree (corpus, then `~/.grok/skills`).
6. `improve`: one MPI perspective per cycle (index below).
7. `adversary`: after a land, when `last_sha != last_adversary_head`.
8. `expand`: after `consecutive_noop >= 3`. Rotate `expand_cursor`. Three expand no-ops -> `prove` on a new tree.

Never ask "should I continue?" after clean MPI cycles.

## PR plan cursor

Bootstrap (this tree) is already on `main`. Remaining slices:

- `pr-2` types and skip-kind taxonomy
- `pr-3` discovery and corpus
- `pr-4` budgets, `filter_skills`, envelope
- `pr-5` CLI
- `pr-6` MCP list/load/why
- `pr-7` Windows and macOS CI
- `pr-8` property tests and fuzz smoke
- `pr-9` Bline consumer notes only (`factory/BLINE_CONSUMER.md`)

`--concurrency 1`. One ready PR.

## MPI perspective list (frozen)

1-based `mpi.next_perspective`. Skip Test Auditor on odd cycles (replace QA on even cycles).

1. QA Engineer
2. Test Auditor
3. Developer
4. End User
5. Maintainer
6. Ops/SRE
7. Security Engineer
8. Product Manager
9. Spec/Contract Compliance
10. Performance Engineer
11. Backward Compatibility
12. Adversarial Tester
13. Architecture Reviewer
14. New Contributor
15. Observability / Error Detective

Improve children: one perspective, no "should I continue?", `REVIEW: 0` plus empty `FILES:` is a no-op.

## Expand rotation

0. Corpus fixture that Bline fires and this crate would miss
1. Property test gap
2. Fuzz smoke
3. Incumbent layout (Vercel / Anthropic)
4. New prove tree
