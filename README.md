# craftbag

Discover, list, load, and explain Agent Skills (`SKILL.md`) for CLI and MCP hosts.

[![CI](https://github.com/craftbag/craftbag/actions/workflows/ci.yml/badge.svg)](https://github.com/craftbag/craftbag/actions/workflows/ci.yml)
[![Security](https://github.com/craftbag/craftbag/actions/workflows/security.yml/badge.svg)](https://github.com/craftbag/craftbag/actions/workflows/security.yml)
[![crates.io](https://img.shields.io/crates/v/craftbag?logo=rust)](https://crates.io/crates/craftbag)
[![docs.rs](https://img.shields.io/docsrs/craftbag?logo=docs.rs)](https://docs.rs/craftbag)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](LICENSE)

[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/14274/badge)](https://www.bestpractices.dev/projects/14274)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/craftbag/craftbag/badge)](https://securityscorecards.dev/viewer/?uri=github.com/craftbag/craftbag)
[![FOSSA Status](https://app.fossa.com/api/projects/custom%2B62586%2Fgithub.com%2Fcraftbag%2Fcraftbag.svg?type=shield&issueType=license)](https://app.fossa.com/projects/custom%2B62586%2Fgithub.com%2Fcraftbag%2Fcraftbag?ref=badge_shield&issueType=license)
[![Release](https://img.shields.io/github/v/release/craftbag/craftbag?logo=github&sort=semver)](https://github.com/craftbag/craftbag/releases/latest)

## What it does

craftbag walks project and home skill trees (`.agents`, plus optional vendor trees for Claude, Cursor, Grok, and Bline). It then:

- catalogs skills (`list`)
- prints one skill body (`load`)
- explains why a skill would activate (`why`)
- checks a package (`validate`)

The same operations exist over MCP stdio (`skills_list`, `skills_load`, `skills_why`, `skills_validate`). Hosts can filter, rank, and load skills without taking a dependency on any one agent product.

## Install

```bash
cargo install --locked craftbag
cargo install --locked craftbag-mcp
```

Library dependency:

```toml
craftbag = "0.1"
```

From git (unreleased tip):

```bash
cargo install --locked --git https://github.com/craftbag/craftbag --bin craftbag
cargo install --locked --git https://github.com/craftbag/craftbag --bin craftbag-mcp
```

MSRV is 1.85.

## Getting started

```bash
git clone https://github.com/craftbag/craftbag
cd craftbag
cargo build -p craftbag --locked
./target/debug/craftbag list
./target/debug/craftbag list --catalog
./target/debug/craftbag why NAME
```

From a crate that already has skills under `.agents` or a vendor tree, add `--vendor claude` (or `cursor`, `grok`, `bline`) to include those roots.

## Demo

![craftbag list](demo/list.gif)

## MCP

`craftbag-mcp` speaks JSON-RPC on stdio. Point your host at the binary and use `skills_list`, `skills_load`, `skills_why`, and `skills_validate`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security reports go to [SECURITY.md](SECURITY.md). Roadmap and governance are in [ROADMAP.md](ROADMAP.md) and [GOVERNANCE.md](GOVERNANCE.md).

## License

Apache-2.0 or MIT. You may choose either. See `LICENSE` and `LICENSE-APACHE`.
