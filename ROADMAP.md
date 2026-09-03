# Roadmap

Plans, not promises. Open an issue before a large change.

## Near term

- Keep the discover, load, why, and validate surfaces stable for
  path-dep hosts.
- Ship 0.1.2 (outline/section load and parse peels) to crates.io and install channels.

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
