# Constitution

Immutable. Changing this file requires a PR titled `chore: amend constitution`. Auto-merge is skipped for that title and for diffs that touch this file.

1. **Public.** README names the product. GitHub description and topics are set. FUNDING is allowed.
2. **Not a parser pitch.** README, crate description, issues, and PR titles must not sell a YAML frontmatter parser.
3. **Apache-2.0 OR MIT.** Keep copyright headers on ported Bline files. No CLA. DCO on every commit (`git commit -s`). `LICENSE` is the MIT text. `LICENSE-APACHE` is Apache-2.0.
4. **Independent org.** Never `blineai/`. Never copy this product into findbug.
5. **Executable tests.** Impl + tests + related docs in the same commit. A feature without a test that would fail if reverted is incomplete. Fixtures fail CI before product modules.
6. **Find many, land few.** One session branch. At most one ready feature PR. Theme batches, not one peel per PR.
7. **Host-neutral API.** No required Bline types in the public crate.
8. **Skip-kind taxonomy is frozen after PR 2.** Additions are a versioned enum variant plus a corpus fixture. Do not add `BudgetOmitted` or `InvocationOff` until they are real skip rows with fixtures.
9. **No auto-merge** for release, publish, or constitution amend PRs. Title skips in the auto-merge step. Path skips for this file stay in that step.
10. **Forbidden trees.** Children must refuse to write under findbug or durable `~/bline`.
11. **Methodology.** Constitution + `/design` + executable tests + `/greenfield-project` + `/execute-plan`. Spec Kit is ritual only.
