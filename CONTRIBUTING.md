# Contributing

## Where to start

- [Good first issues](https://github.com/craftbag/craftbag/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22)
- [Help wanted](https://github.com/craftbag/craftbag/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22)

Open an issue before a large change. Small, tested fixes can go
straight to a pull request.

## Local gate

The commands in `AGENTS.md` must pass on your workspace before you
open a pull request. In short:

```bash
make check
```

Every commit needs a Developer Certificate of Origin trailer:

```bash
git commit -s
```

The sign-off email is `git config user.email`. The DCO workflow skips
bot commits and merge commits.

## Pull requests

Use the pull request template. One ready pull request at a time.
Commits on `main` squash through the required `CI` check.

PR titles must be a conventional type (`feat`, `fix`, `docs`, `ci`,
`chore`, `test`, `refactor`, `perf`, `build`, `style`, `revert`).
The Semantic PR Title check enforces that.

## License

This project is dual-licensed under Apache-2.0 or MIT. You may choose
either. See `LICENSE` (MIT) and `LICENSE-APACHE`.

## Conduct

See `CODE_OF_CONDUCT.md`. Security reports go to `SECURITY.md`, not
a public issue.
