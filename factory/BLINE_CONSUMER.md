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

`SkillSource::as_str()` for extra-path is `extra`. Lib serde still emits
`extraPath`. CLI/MCP list JSON and why JSON `source` use the wire token
(`extra`, `claude`) so they match list XML/TSV. Use
`SkillSource::wire_name()` for a stable display token
and `SkillSource::from_host_token` to accept Bline list / TUI tokens
(`user`, `agents`, `extra` / `extraPath` / `extra_path` / `config`, plus vendor names
`bline` / `claude` / `cursor` / `grok`, or the on-disk form `.claude`).
Vendor spellings match `parse_vendor_roots` (`.Claude` / `Claude` are
`claude`). `project` and `community` have no v1 variant. The host keeps
those.

CLI `--vendor` and MCP `vendor` reject unknown tokens
(`SkillSource::parse_vendor_roots`). A typo is an error, not an empty
catalog. `extra` is `--path` / `paths`, not a vendor name.

CLI `list --format` uses the same tokens as MCP `skills_list`
(`json`, `xml`, `catalog`, `watch`). `watch-dirs` and `watch_dirs`
are the `--watch-dirs` flag name (same walk). The older `--json` /
`--xml` / `--catalog` / `--watch-dirs` flags stay. CLI `list --json` / `list --xml` /
`list --catalog`, MCP `skills_list` (json, xml, catalog, or watch), and
`why` JSON `loaded` rows include
`description` plus `user_invocable`, `disable_model_invocation`, and
`argument_hint` so a host can build a slash palette or prompt catalog
without re-parsing SKILL.md. Omitted `argument_hint` is JSON `null`
and an empty `<argument_hint>` tag. List and why JSON both serialize
`SkillSummary` so those keys cannot drift. CLI `load` and MCP
`skills_load` print `Argument hint: …` in the envelope when
frontmatter `argument-hint` is set (same text as list/why JSON).
Omitted hint is no line. Folded newlines become spaces so the field
stays one line. Catalog markdown is one list item per skill: newlines in a
literal `|` or folded `>` description become spaces (JSON and XML keep
the raw description). List XML also emits `<source>` (`agents`, `user`, `extra`,
or the vendor token), matching `SkillSource::as_str`. Keys stay snake_case on those wires (same as frontmatter),
not Skill's camelCase serde. Omitted flags on old why JSON keep the
pre-90 defaults (`user_invocable` true, `disable_model_invocation`
false). `filter_skills` / `why.activation` still use only
`disable_model_invocation` for auto-inject.

Set `DiscoveryOptions.ascii_names` (CLI `--ascii-names`, MCP
`ascii_names`) until Bline chooses Unicode / NFKC as product policy.
Default discover still loads `café` / `перевод`. With the option those
names are a `parse_error` skip (`lowercase alphanumeric and hyphens
only`) and do not appear in `skills`.

CLI `list --watch-dirs` and MCP `skills_list format=watch` print that
same list (one path per line) without loading SKILL.md. An extra-path
that is a SKILL.md file is listed; a FIFO or other non-file is omitted.

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
| `incumbent/claude-user` | `HOME` + `--vendor claude` | load `home-note` from `~/.claude/skills` | load `home-note` | expected (source wire: vendor vs Claude) |
| `incumbent/vercel-npx` | extra path | load `deploy-hint` via `skills/` | load `deploy-hint` via `skills/` | match |
| `incumbent/vercel-npx/skills` | extra path is the `skills/` collection | load `deploy-hint` | load `deploy-hint` | match |

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
