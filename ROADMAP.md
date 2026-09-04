# Roadmap

Plans, not promises. Open an issue before a large change.

## Near term

- Keep the discover, load, why, and validate surfaces stable for
  path-dep hosts.
- v0.1.2 is on crates.io (`craftbag`, `craftbag-cli`, `craftbag-mcp`)
  with Homebrew and Scoop install channels (outline/section load and
  parse peels).

## Medium term

- Hosts consume the library as a crates.io `0.1` pin without copying
  the walk (path-dep and git pins remain supported).
- Tighten release automation so a tested tree is promoted without a
  second full product compile on the tag (already closer after
  compile-once CI).

## Long term

- Grow a second reviewer so `code_owner_review` can turn on without
  a merge deadlock.
- Optional docs site after the public crate and install channels.
