# Child report

Every volume child ends with exactly this block. Parent discards everything else.

```text
HEAD: <sha>
PR: <number or none>
FILES: <paths>
TESTS: <command> -> <N passed | FAIL: name>
REVIEW: <0 must-fix | N must-fix: one-line each>
NEXT: <one parent action>
```

`NEXT` is one of: `merge-pr N` | `dispatch-main-ci` | `advance-cursor` | `switch-expand` | `reap-lease` | `human-gate:<kind>` | `next-cycle`.
`dispatch-main-ci` is only when that tree never ran the PR matrix.
A green squash of an up-to-date PR must not start another compile.
