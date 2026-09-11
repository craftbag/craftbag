# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-11

`load --json` and `why` misses match the rest of the CLI, leftover
user-dir files no longer look like a skill named `skills`, and library
`discover` returns the report directly.

### Breaking

- **`discover` returns `DiscoveryReport`.** On 0.1.2 it returned
  `Result<DiscoveryReport, Error>`. The walk does not fail that way,
  so drop `?` (or the `Ok` match) and use the report ([#368](https://github.com/craftbag/craftbag/pull/368)).

  ```rust
  let report = craftbag::discover(&cwd, &opts);
  ```

### Fixed

- **`craftbag load --json` printed the envelope, not JSON.** Success
  is now an object with `name`, `path`, `source`, and `text` ([#368](https://github.com/craftbag/craftbag/pull/368)).
- **`why NAME` missed with exit 1.** A missing skill now exits 2,
  same as `load`. Flag and tool errors stay 1 ([#368](https://github.com/craftbag/craftbag/pull/368)).
- **Empty `list` / `why` printed nothing.** Stderr now says which
  roots were watched. Stdout stays empty. Exit stays 0 ([#368](https://github.com/craftbag/craftbag/pull/368)).
- **A refused `--path` looked like a skipped skill of that name.**
  The miss is `unknown skill: NAME; refused --path / paths: ...`
  and still peels `error_kind=unreadable` ([#368](https://github.com/craftbag/craftbag/pull/368)).
- **`---x` opened or closed frontmatter.** Only a line that is
  exactly `---` is a fence. A close of `---x` no longer prepends
  `x` to the body ([#368](https://github.com/craftbag/craftbag/pull/368)).
- **A YAML list under a host key (`tags:`) failed parse.** Those
  lists are ignored. Only `triggers` collects items. A list under
  a known scalar (`license:`) is still invalid YAML ([#368](https://github.com/craftbag/craftbag/pull/368)).
- **MCP `ascii_names: false` kept launch `--ascii-names`.** A
  present false now overrides. Omitted still uses the launch
  default ([#368](https://github.com/craftbag/craftbag/pull/368)).
- **Leftover `user_dir/skills/SKILL.md` looked like a package
  named `skills`.** With no sibling packages it is `root_file`.
  `load` / `why` of `skills` stay `unknown_skill`. Extra-path
  leftover already did this ([#370](https://github.com/craftbag/craftbag/pull/370)).

### Changed

- **Getting started works on a clone of this repo.** The first
  README commands pass `--path demo/workspace/.agents/skills`.
  `list` from the crate root is empty (no project `.agents`). The
  walk-through includes `load --outline` and `load --section`.
  `craftbag-mcp --help` names those fields ([#350](https://github.com/craftbag/craftbag/pull/350)).

### Upgrade

Library hosts that want this cut must move the pin. `craftbag = "0.1"`
stays on 0.1.2:

```toml
craftbag = "0.2"
```

```bash
cargo update -p craftbag
```

CLI and MCP:

```bash
brew upgrade craftbag/tap/craftbag
# or
cargo install --locked craftbag-cli
cargo install --locked craftbag-mcp
```

Compare: https://github.com/craftbag/craftbag/compare/v0.1.2...v0.2.0

## [0.1.2] - 2026-09-03

Hosts can load one SKILL.md heading or an outline of headings instead
of the whole body. Parse and MCP miss peels match real SKILL.md trees
and closed pipes no longer panic the CLI.

### Added

- **Outline and section load.** `craftbag load --outline` lists heading
  keys and token hints. `load --section KEY` prints one heading body.
  MCP `skills_load` has the same `outline` and `section` fields. Does
  not dump `scripts/` or `references/` file bodies
  ([#333](https://github.com/craftbag/craftbag/pull/333)).

### Fixed

- **Fenced code is not a heading.** `#` lines inside fenced blocks no
  longer split outline or section bodies
  ([#347](https://github.com/craftbag/craftbag/pull/347)).
- **YAML quotes and escapes.** Unquoted text that ends in a quote keeps
  that character. Single-quoted `''` and double-quoted `\"` decode as
  YAML ([#347](https://github.com/craftbag/craftbag/pull/347)).
- **Strict validate and colons in triggers.** A zero-indent trigger item
  that contains a colon is no longer rejected as an unknown key
  ([#347](https://github.com/craftbag/craftbag/pull/347)).
- **MCP section misses peel cleanly.** Unknown section sets
  `error_kind`, and `outline` plus `section` together is rejected before
  walking the tree
  ([#347](https://github.com/craftbag/craftbag/pull/347)).
- **Broken pipe and bad stdin.** `craftbag list | head` exits 0.
  `craftbag-mcp` replies `-32700` on a non-UTF-8 stdin line and keeps
  the session ([#347](https://github.com/craftbag/craftbag/pull/347)).
- **Stable sibling order.** Extra-path packages list in file-name order
  ([#347](https://github.com/craftbag/craftbag/pull/347)).
- **Install docs name the CLI crate.** README `cargo install` /
  `cargo build` lines use `craftbag-cli`
  ([#347](https://github.com/craftbag/craftbag/pull/347)).
- **Faster crates.io version bumps.** Publish no longer waits 10 minutes
  between version bumps of crate names that already exist
  ([#332](https://github.com/craftbag/craftbag/pull/332),
  [#347](https://github.com/craftbag/craftbag/pull/347)).

### Upgrade

Library hosts (`craftbag = "0.1"`):

```bash
cargo update -p craftbag
```

CLI and MCP:

```bash
brew upgrade craftbag/tap/craftbag
# or
cargo install --locked craftbag-cli
cargo install --locked craftbag-mcp
```

Compare: https://github.com/craftbag/craftbag/compare/v0.1.1...v0.1.2

## [0.1.1] - 2026-08-28

Hosts that pass a collection directory now load leftover sibling
packages next to a child `skills/` tree, and a tight catalog budget
stays inside its byte limit. You can install both binaries without a
Rust toolchain.

### Added

- **Install without compiling.** `brew install craftbag/tap/craftbag`
  or Scoop (`scoop bucket add craftbag
  https://github.com/craftbag/scoop-bucket` then `scoop install
  craftbag/craftbag`) puts `craftbag` and `craftbag-mcp` on PATH.
  GitHub Release archives ship both binaries for macOS (Apple Silicon
  and Intel), Linux x64, and Windows x64 ([#326](https://github.com/craftbag/craftbag/pull/326)).
- **MCP launch defaults.** `craftbag-mcp --path`, `--vendor`, and
  `--user-dir` apply to every tool call unless the host overrides
  them. `--help` and `--version` work; the binary used to ignore argv ([#320](https://github.com/craftbag/craftbag/pull/320), [#319](https://github.com/craftbag/craftbag/pull/319)).
- **Getting started that actually lists skills.** README now covers
  CLI, vendor trees, MCP host config, and library embedders, plus a
  demo workspace you can run from a clone ([#316](https://github.com/craftbag/craftbag/pull/316), [#318](https://github.com/craftbag/craftbag/pull/318), [#319](https://github.com/craftbag/craftbag/pull/319)).

### Fixed

- **Sibling packages next to `skills/` were hidden.** A host `--path`
  at a collection that had both `skills/` and leftover packages
  (`wanted/`, `other/`) only walked `skills/`. Those siblings now load ([#323](https://github.com/craftbag/craftbag/pull/323)).
- **Catalog hard clamp could exceed `catalog_max_chars` by 2 bytes.**
  The trailing ellipsis now reserves its full UTF-8 width, so a tight
  budget stays inside the limit ([#323](https://github.com/craftbag/craftbag/pull/323)).
- **A missing `--path` or `--user-dir` looked empty.** A host-asked
  directory that does not exist is now an `Unreadable` skip that names
  the flag. Named `load` no longer says unknown skill ([#325](https://github.com/craftbag/craftbag/pull/325)).
- **Miss-path errors name the next step.** Load and validate say what
  to change when the path is missing ([#324](https://github.com/craftbag/craftbag/pull/324)).
- **Published `craftbag` crate no longer ships factory/, demo/, or
  brand/.** The crates.io tarball is compile, license, and docs ([#324](https://github.com/craftbag/craftbag/pull/324)).

### Upgrade

Library hosts (`craftbag = "0.1"`):

```bash
cargo update -p craftbag
```

After this crate you can drop a host walk that only existed to pick
up sibling packages next to `skills/`, and you can stop clipping the
catalog after `format_catalog`. The pin stays `0.1`; Cargo resolves
0.1.1.

CLI and MCP:

```bash
brew upgrade craftbag/tap/craftbag
# or
cargo install --locked craftbag-cli
cargo install --locked craftbag-mcp
```

Compare: https://github.com/craftbag/craftbag/compare/v0.1.0...v0.1.1

## [0.1.0] - 2026-08-28

First public release.

### Added

- Library that discovers and loads Agent Skills (`SKILL.md`) for CLI and MCP hosts
- `craftbag` CLI: `list`, `load`, `why`, and `validate`
- `craftbag-mcp` stdio server with `skills_list`, `skills_load`, `skills_why`, and `skills_validate`
- Dual `Apache-2.0 OR MIT` license
