# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `craftbag load --outline` and `load --section KEY` print heading
  keys or one SKILL.md section instead of the whole body. MCP
  `skills_load` has the same `outline` and `section` fields. Does
  not dump `scripts/` or `references/` file bodies.

### Fixed

- Release publish no longer waits 10 minutes between version bumps of
  crates that already exist on crates.io

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
