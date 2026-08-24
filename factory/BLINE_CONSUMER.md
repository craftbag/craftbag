# Bline consumer spike

Handoff only. This file does not change Bline. Do not open a Bline
issue from this tree. Do not write under findbug or durable `~/bline`.

## Goal

A later Bline change can path-depend on this crate, compare skip
kinds on one tree, then (if wanted) re-export `Skill` / `SkipKind`.

`load` and `why` already exist here (`craftbag` CLI and `craftbag-mcp`).

## Path-dep (separate worktree)

Use a Bline worktree that is not findbug and not the durable
`~/bline` install. Example Cargo.toml fragment in that worktree:

```toml
# package name is craftbag; path is a clone of this repo
bline-skills = { path = "/path/to/craftbag", package = "craftbag" }
```

Later, if published: `craftbag = "0.1"`. This repo stays
`publish = false` until a launch gate.

## Intended shim

Bline owns host paths. craftbag stays host-neutral.

| Bline input | craftbag `DiscoveryOptions` |
|-------------|-----------------------------|
| `config_dir()/bline/skills` | `user_skills_dir` (`SkillSource::User`) |
| opt-in `.bline/skills` | `vendor_roots` includes `"bline"` |
| extra roots | `paths` (`SkillSource::ExtraPath`) |
| repo `.agents/skills` | walked from cwd (no `Project` variant) |

There is no `SkillSource::Project` in v1. Repo-local `.agents/skills`
is `Agents`.

A thin Bline module can re-export `Skill`, `SkipKind`, `DiscoveryReport`,
and `WhyReport` if wire names stay aligned (`camelCase` structs,
`snake_case` skip kinds). Do not require Bline types in this crate.

## Compare on one tree

On the same fixture tree (this repo `tests/corpus/` first):

1. `craftbag why --json --path <tree>` (and `--user-dir` / `--vendor`
   as needed).
2. Bline doctor output for the same tree (whatever Bline calls that
   command today).
3. Diff skip `kind` + `winnerPath` + path.

Activation (`why.activation`) is a craftbag doctor. Bline may not have
the same field. Compare skip rows first.

## Skip-kind parity

craftbag v1 `SkipKind` (frozen). Wire names match Bline
`SkillSkipKind` as extracted:

| craftbag | Wire | Bline parity |
|----------|------|--------------|
| `Unreadable` | `unreadable` | IO / symlink escape |
| `ParseError` | `parse_error` | Frontmatter / name rules |
| `NameDirectoryMismatch` | `name_directory_mismatch` | `name` != parent dir |
| `NameCollision` | `name_collision` | first name wins; `winnerPath` set |
| `RootFile` | `root_file` | loose `SKILL.md` in a skills root |

Not skip kinds in v1 (silent or activation-only):

| Name | Where it lives |
|------|----------------|
| `BudgetOmitted` | `why.activation` / `filter_skills` only |
| `Disabled` | `DiscoveryOptions.disabled` (no skip row) |
| `VendorDenylist` | Cursor list is silent |
| `InvocationOff` | `disable_model_invocation` is activation, not a skip |

Do not add those four as `SkipKind` variants until they have corpus
fixtures.

## Out of scope here

- Editing Bline source.
- Filing the Bline consumer issue.
- crates.io publish.
- MCP Registry / Smithery listings.
