# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Published `craftbag` crate no longer ships factory/, demo/, or brand/

### Added

- `craftbag-mcp --help` and `--version` (stdio server used to ignore argv)
- README getting started for CLI, vendor trees, MCP hosts, and library
  embedders, plus a runnable demo-workspace command
- `craftbag-mcp --path` / `--vendor` / `--user-dir` launch defaults so a
  host config can pin the walk without repeating it on every tool call

## [0.1.0] - 2026-08-28

First public release.

### Added

- Library that discovers and loads Agent Skills (`SKILL.md`) for CLI and MCP hosts
- `craftbag` CLI: `list`, `load`, `why`, and `validate`
- `craftbag-mcp` stdio server with `skills_list`, `skills_load`, `skills_why`, and `skills_validate`
- Dual `Apache-2.0 OR MIT` license
