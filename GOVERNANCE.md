# Governance

## Maintainer

Sebastien Tardif (`@SebTardif`) is the current maintainer.

## Decisions

Day-to-day changes land through pull requests. The required check is
`CI`. The maintainer squash-merges when that check is green.

Design changes that alter public types, skip kinds, or the host
contract should be discussed in an issue first.

## Releases

The maintainer cuts releases. There is no automatic publish. A later
release job may compile once for signing; it must not re-run the PR
test matrix.

## Escalation

Use GitHub Issues for product questions. Security reports go through
`SECURITY.md` (private advisory). Conduct reports follow
`CODE_OF_CONDUCT.md`.
