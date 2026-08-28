# Roadmap

Plans, not promises. Open an issue before a large change.

## Near term

- Keep the discover, load, why, and validate surfaces stable for
  path-dep hosts.
- Finish launch-prep files that do not advertise the repo
  (`CONTRIBUTING.md`, `SECURITY.md`, this file).
- First tagged release after an explicit launch (crates.io stays
  unpublished until then).

## Medium term

- Hosts consume the library as a path or git dependency without
  copying the walk.
- Release automation that promotes a tested tree (no second
  `cargo test` on the tag).

## Long term

- Grow a second reviewer so `code_owner_review` can turn on without
  a merge deadlock.
- Optional docs site and install channels after the first public
  release.
