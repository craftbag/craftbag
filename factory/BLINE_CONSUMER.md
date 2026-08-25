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

`SkillSource::as_str()` for extra-path is `extra`. Serde still emits
`extraPath`. Use `SkillSource::wire_name()` for a stable display token
and `SkillSource::from_host_token` to accept Bline list / TUI tokens
(`user`, `agents`, `extra` / `extraPath` / `config`, plus vendor names
`bline` / `claude` / `cursor` / `grok`, or the on-disk form `.claude`).
Vendor spellings match `parse_vendor_roots` (`.Claude` / `Claude` are
`claude`). `project` and `community` have no v1 variant. The host keeps
those.

CLI `--vendor` and MCP `vendor` reject unknown tokens
(`SkillSource::parse_vendor_roots`). A typo is an error, not an empty
catalog. `extra` is `--path` / `paths`, not a vendor name.

CLI `list --json` / `list --xml`, MCP `skills_list` (json or xml), and
`why` JSON `loaded` rows include `user_invocable` and
`disable_model_invocation` so a host can build a slash palette without
re-parsing SKILL.md. Keys stay snake_case on those wires (same as
frontmatter), not Skill's camelCase serde. `filter_skills` /
`why.activation` still use only `disable_model_invocation` for
auto-inject.

Set `DiscoveryOptions.ascii_names` (CLI `--ascii-names`, MCP
`ascii_names`) until Bline chooses Unicode / NFKC as product policy.
Default discover still loads `café` / `перевод`. With the option those
names are a `parse_error` skip (`lowercase alphanumeric and hyphens
only`) and do not appear in `skills`.

Hot reload should watch `watch_dirs(cwd, opts)`, not a copy of
`walk_cwd_to_git_root`. That list is the same user dir, vendor
`.{name}/skills` trees, cwd-to-git `.agents/skills` roots, and extra
paths that `discover` walks: a named extra-path package is the package
dir only; a collection is `dir` plus `dir/skills`. An extra-path
`SKILL.md` file is listed only when it is a regular file (or a symlink
to one). A FIFO, socket, or device is omitted, same as discover.
Escaped or unreadable extra `skills/` is omitted (discover falls back
to `dir/`).

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

## Corpus skip-row compare (2026-08-24)

Issue [#54](https://github.com/craftbag/craftbag/issues/54). Isolated
`HOME`. Corpus copied to `/tmp/prove-corpus` so a Bline cwd walk does
not climb the craftbag git root. craftbag: `craftbag why --json`.
Bline: `discover_skills_for_cwd_report` (the doctor skip source).
`bline skills list --json` does not emit skip rows.

| Tree | How | craftbag | Bline | Class |
|------|-----|----------|-------|-------|
| `lowercase-skill-md` | cwd (`.agents/skills`) | load `lc-pack` | load `lc-pack` | match |
| `agentskills/minimal-valid` | extra path | load `minimal-valid` | load `minimal-valid` | match |
| `agentskills/package-full` | extra path | load `package-full` | load `package-full` | match |
| `agentskills/name-mismatch` | extra path | skip `name_directory_mismatch` `good-name` | same kind, name, path | match |
| `agentskills/invalid-name` | extra path | skip `parse_error` name `Bad_Name` | same kind; `name` is null; detail is ASCII-only | expected (peek + name policy wording) |
| `root-file` | extra path as package dir | skip `name_directory_mismatch` `loose` | same | expected (this extra path is a package, not a skills-root loose file) |
| `collision/a` + `collision/b` | two extra paths | load `a/foo`; skip `name_collision` `b/foo` `winnerPath`=`a/foo` | same | match |
| `incumbent/claude-project` | cwd + `--vendor claude` | load `pdf-helper` source vendor claude | load `pdf-helper` | expected (source wire: vendor vs Claude) |
| `incumbent/vercel-npx` | extra path | load `deploy-hint` via `skills/` | load `deploy-hint` via `skills/` | match |

`--path` on a project root (`lowercase-skill-md`) does not walk
`.agents/skills` (extra-path package/collection rules). Bline cwd
walk does. The compare used cwd for that tree.

Unexpected skip-kind or `winnerPath` mismatches: none.

## Out of scope here

- Editing Bline source.
- Bline path-dep (that is
  [blineai/bline#3497](https://github.com/blineai/bline/issues/3497)).
- crates.io publish.
- MCP Registry / Smithery listings.
