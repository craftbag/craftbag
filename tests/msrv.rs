//! Local rustc must match CI. A newer default toolchain can accept
//! let-chains that fail the 1.85 job.

fn repo_file(rel: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(root.join(rel))
        .unwrap_or_else(|e| panic!("read {rel}: {e}"))
        .replace("\r\n", "\n")
}

#[test]
fn rust_toolchain_toml_matches_cargo_msrv_and_ci() {
    let toolchain = repo_file("rust-toolchain.toml");
    assert!(
        toolchain.contains("channel = \"1.85\""),
        "rust-toolchain.toml must pin 1.85: {toolchain}"
    );
    assert!(
        toolchain.contains("profile = \"minimal\""),
        "rust-toolchain.toml must use profile = minimal so a first clone does not fetch rust-docs: {toolchain}"
    );
    assert!(
        toolchain.contains("\"rustfmt\"") && toolchain.contains("\"clippy\""),
        "minimal profile must still list rustfmt and clippy for make check: {toolchain}"
    );
    let ci = repo_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("toolchain: \"1.85\""),
        "ci.yml rust jobs must stay on 1.85"
    );
    // Root-only used to miss a member crate advertising a newer rust-version.
    for cargo in [
        "Cargo.toml",
        "crates/craftbag-cli/Cargo.toml",
        "crates/craftbag-mcp/Cargo.toml",
    ] {
        let text = repo_file(cargo);
        assert!(
            text.contains("rust-version = \"1.85\""),
            "{cargo} rust-version must stay 1.85"
        );
    }
}

/// Isolated HOME fixtures (`incumbent/*-user`) are real directories.
/// rustup/cargo write `$HOME/.cargo` and `$HOME/.rustup` if a command
/// runs with HOME pointed at the fixture. A first clone must not be
/// able to `git add` those trees.
#[test]
fn corpus_home_fixtures_ignore_rustup_writes() {
    let gitignore = repo_file(".gitignore");
    for pat in ["tests/corpus/**/.cargo/", "tests/corpus/**/.rustup/"] {
        assert!(
            gitignore.lines().any(|l| l.trim() == pat),
            ".gitignore must ignore rustup/cargo writes under isolated HOME fixtures: {pat}"
        );
    }
    let ci = repo_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("- '.gitignore'"),
        "rust path-filter must include .gitignore so an ignore-line delete runs this lock"
    );
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("git")
        .args(["ls-files", "tests/corpus"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracked = String::from_utf8_lossy(&output.stdout);
    for line in tracked.lines() {
        let junk = line.contains("/.cargo/")
            || line.contains("/.rustup/")
            || line.ends_with("/.cargo")
            || line.ends_with("/.rustup");
        assert!(
            !junk,
            "corpus must not track rustup/cargo HOME writes: {line}"
        );
    }
}

/// Commands a first clone actually copy-pastes: the AGENTS.md bash fence.
fn agents_local_gate_fence(agents: &str) -> Vec<&str> {
    let after_heading = agents
        .split("Local gate before every commit")
        .nth(1)
        .expect("AGENTS.md must have a Local gate heading");
    let after_open = after_heading
        .split("```bash\n")
        .nth(1)
        .expect("AGENTS.md local gate must be a ```bash fence");
    let body = after_open
        .split("```")
        .next()
        .expect("AGENTS.md local-gate fence must close");
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Recipes under the `check:` target. A Makefile that exists but lists
/// fewer lines than AGENTS.md is first-PR drift.
fn makefile_check_commands(makefile: &str) -> Vec<&str> {
    let after = makefile
        .split("check:\n")
        .nth(1)
        .expect("Makefile must have a check: target");
    let mut cmds = Vec::new();
    for line in after.lines() {
        if let Some(rest) = line.strip_prefix('\t') {
            let cmd = rest.trim();
            if !cmd.is_empty() && !cmd.starts_with('#') {
                cmds.push(cmd);
            }
            continue;
        }
        if line.chars().next().is_some_and(|c| !c.is_whitespace()) {
            break;
        }
    }
    cmds
}

/// Local AGENTS.md fence, Makefile `check`, and hosted CI must run the
/// same commands. Substring checks alone miss an extra fence line.
#[test]
fn local_gate_commands_run_in_ci() {
    let agents = repo_file("AGENTS.md");
    let makefile = repo_file("Makefile");
    let ci = repo_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("- 'AGENTS.md'"),
        "rust path-filter must include AGENTS.md so a local-gate edit runs this test"
    );
    assert!(
        ci.contains("- 'Makefile'"),
        "rust path-filter must include Makefile so a check-target edit runs this test"
    );
    assert!(
        agents.contains("`make check`"),
        "AGENTS.md must name make check next to the local-gate fence"
    );
    let expected = [
        "cargo fmt --check",
        "cargo clippy --locked --workspace --all-targets -- -D warnings",
        "cargo nextest run --locked --workspace",
        "cargo test --locked --workspace --doc",
        "bash factory/scripts/deny-check.sh",
        "bash factory/scripts/assert-stealth.sh craftbag/craftbag",
        "bash factory/scripts/write-ledger.sh --self-test",
    ];
    let fence = agents_local_gate_fence(&agents);
    let make_cmds = makefile_check_commands(&makefile);
    assert_eq!(
        fence.len(),
        expected.len(),
        "AGENTS.md local-gate fence has {} commands, lock has {}",
        fence.len(),
        expected.len()
    );
    assert_eq!(
        make_cmds.len(),
        expected.len(),
        "Makefile check has {} commands, lock has {}",
        make_cmds.len(),
        expected.len()
    );
    assert_eq!(
        fence, expected,
        "AGENTS.md local-gate fence drifted from the lock"
    );
    assert_eq!(
        make_cmds, expected,
        "Makefile check drifted from the AGENTS.md local-gate fence"
    );
    for cmd in expected {
        assert!(
            ci.contains(cmd),
            "ci.yml must run {cmd}; a local-only gate command is not CI"
        );
    }
}

fn ci_job_body(ci: &str, job: &str) -> String {
    let mut in_jobs = false;
    let mut in_job = false;
    let mut body = String::new();
    for line in ci.lines() {
        if !in_jobs {
            if line == "jobs:" {
                in_jobs = true;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("  ") {
            if !rest.starts_with(' ') && !rest.starts_with('#') && rest.ends_with(':') {
                let name = rest.trim_end_matches(':');
                if in_job {
                    break;
                }
                in_job = name == job;
                continue;
            }
        }
        if in_job {
            body.push_str(line);
            body.push('\n');
        }
    }
    assert!(!body.is_empty(), "ci.yml must have job {job}");
    body
}

fn rust_cache_step(job_body: &str) -> String {
    let lines: Vec<&str> = job_body.lines().collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Swatinem/rust-cache@") {
            let mut s = i;
            while s > 0 && !lines[s].trim_start().starts_with("- ") {
                s -= 1;
            }
            start = Some(s);
            break;
        }
    }
    let start = start.expect("job must have a rust-cache step");
    let indent = lines[start].chars().take_while(|c| *c == ' ').count();
    let mut out = String::new();
    for (j, line) in lines.iter().enumerate().skip(start) {
        if j > start {
            let next_indent = line.chars().take_while(|c| *c == ' ').count();
            if line.trim_start().starts_with("- ") && next_indent <= indent {
                break;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// rust-cache on lint/test/fuzz/windows/macos must save only on main.
/// PR jobs still restore. Factory PR cache writes evict the main restore.
#[test]
fn rust_cache_save_if_main_only() {
    let ci = repo_file(".github/workflows/ci.yml");
    let save_if = "save-if: ${{ github.ref == 'refs/heads/main' }}";
    for job in ["lint", "test", "fuzz-smoke", "test-windows", "test-macos"] {
        let body = ci_job_body(&ci, job);
        let step = rust_cache_step(&body);
        assert!(
            step.contains(save_if),
            "{job} rust-cache must set {save_if} so only main writes: {step}"
        );
    }
    let fuzz = rust_cache_step(&ci_job_body(&ci, "fuzz-smoke"));
    assert!(
        fuzz.contains("shared-key: fuzz"),
        "fuzz-smoke rust-cache must keep shared-key: fuzz: {fuzz}"
    );
    assert!(
        fuzz.contains("cache-on-failure: true"),
        "fuzz-smoke rust-cache must keep cache-on-failure: true: {fuzz}"
    );
}

/// rust-toolchain does not install nextest, cargo-deny, or gh. A first
/// clone that only rustups 1.85 cannot run the documented local gate.
/// Latest nextest/cargo-deny do not build on rustc 1.85.
#[test]
fn local_gate_names_extra_tools() {
    let agents = repo_file("AGENTS.md");
    for needle in ["cargo-nextest", "cargo-deny", "`gh`"] {
        assert!(
            agents.contains(needle),
            "AGENTS.md must name extra gate tool {needle} (not rust-toolchain)"
        );
    }
    assert!(
        agents.contains("prebuilt"),
        "AGENTS.md must send a first clone to prebuilt binaries, not cargo install on 1.85"
    );
    assert!(
        !agents.contains("cargo install cargo-nextest")
            && !agents.contains("cargo install cargo-deny"),
        "AGENTS.md must not cargo-install nextest/deny on this crate's rustc 1.85"
    );
    // README #install is `cargo install --locked cargo-deny`. That fails
    // (or needs a newer rustc) on this crate's MSRV 1.85.
    assert!(
        !agents.contains("github.com/EmbarkStudios/cargo-deny#install"),
        "AGENTS.md cargo-deny must not link README #install (that is cargo install)"
    );
    assert!(
        agents.contains("nexte.st/docs/installation/pre-built-binaries"),
        "AGENTS.md nextest must link prebuilt binaries"
    );
    assert!(
        agents.contains("embarkstudios.github.io/cargo-deny/cli")
            || agents.contains("github.com/EmbarkStudios/cargo-deny/releases"),
        "AGENTS.md must send cargo-deny to prebuilt binaries, not cargo install"
    );
}

/// Bline host notes must name the SkillMiss peel that landed in #132-#134.
#[test]
fn bline_consumer_host_table_names_skill_miss() {
    let notes = repo_file("factory/BLINE_CONSUMER.md");
    let ci = repo_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("- 'factory/BLINE_CONSUMER.md'"),
        "rust path-filter must include factory/BLINE_CONSUMER.md so a host-table edit runs this test"
    );
    assert!(
        notes.lines().any(|l| {
            l.contains('|')
                && l.contains("SkillMiss.error")
                && l.contains("error_kind")
                && l.contains("SkillMiss.path")
        }),
        "BLINE_CONSUMER host table must name SkillMiss.error, error_kind, and path"
    );
}

/// CLI `validate --help` and MCP `skills_validate` must both name the
/// package directory and ValidationReport. PR 263 added the MCP sibling;
/// a leftover host that reads only one surface must still see the same
/// success shape.
#[test]
fn validate_help_and_mcp_describe_package_dir_and_report() {
    let notes = repo_file("factory/BLINE_CONSUMER.md");
    assert!(
        notes.contains("MCP `skills_validate`")
            && notes.contains("ValidationReport")
            && notes.contains("package directory"),
        "BLINE_CONSUMER must name MCP skills_validate package dir + ValidationReport"
    );
    assert!(
        notes.lines().any(|l| {
            l.contains('|') && l.contains("MCP `skills_validate`") && l.contains("ValidationReport")
        }),
        "BLINE_CONSUMER host table must keep the MCP skills_validate row"
    );
    let cli = repo_file("crates/craftbag-cli/src/main.rs");
    let mcp = repo_file("crates/craftbag-mcp/src/main.rs");
    for (label, src) in [("CLI validate", &cli), ("MCP skills_validate", &mcp)] {
        assert!(
            src.contains("package directory") && src.contains("ValidationReport"),
            "{label} must name package dir + ValidationReport: leftover analog of load --help / skills_load"
        );
        assert!(
            src.contains("no error_kind"),
            "{label} must say success has no error_kind"
        );
    }
}

/// A host adding a SkillSummary field must see the sibling lock here,
/// not only in why.rs. Catalog stays cheap. Load is the text envelope.
#[test]
fn bline_consumer_names_skill_summary_sibling_lock() {
    let notes = repo_file("factory/BLINE_CONSUMER.md");
    assert!(
        notes.contains("skill_summary_json_keys_have_list_xml_siblings"),
        "BLINE_CONSUMER must name the SkillSummary sibling-lock test"
    );
    assert!(
        notes.contains("list JSON") && notes.contains("why JSON") && notes.contains("list XML"),
        "BLINE_CONSUMER must name list/why JSON + list XML as SkillSummary siblings"
    );
    assert!(
        notes.to_ascii_lowercase().contains("catalog stays cheap"),
        "BLINE_CONSUMER must say catalog stays cheap for a new SkillSummary field"
    );
    assert!(
        notes.contains("text envelope"),
        "BLINE_CONSUMER must say load is the text envelope, not SkillSummary JSON"
    );
    assert!(
        notes.contains("load --help")
            && notes.contains("argument-hint")
            && notes.contains("when-to-use")
            && notes.contains("triggers")
            && notes.contains("allowed-tools")
            && notes.contains("license")
            && notes.contains("compatibility")
            && notes.contains("metadata"),
        "BLINE_CONSUMER must name CLI load --help / MCP skills_load envelope fields"
    );
}

/// why --json success is WhyReport. list already names `{ skills, skips }`.
#[test]
fn bline_consumer_names_why_json_success_keys() {
    let notes = repo_file("factory/BLINE_CONSUMER.md");
    assert!(
        notes.contains("{ loaded, skips, activation }"),
        "BLINE_CONSUMER must name why --json success keys like list {{ skills, skips }}"
    );
}

/// `watch_dirs_omits_ignored_extra_path` and watch_dirs rustdoc omit
/// ignore prefixes. Host notes must not keep a second paragraph that
/// says `watch_dirs` is unchanged.
#[test]
fn bline_consumer_watch_dirs_omits_ignore_prefixes() {
    let notes = repo_file("factory/BLINE_CONSUMER.md");
    assert!(
        notes.contains("`watch_dirs` omits the same prefixes"),
        "BLINE_CONSUMER must say watch_dirs omits ignore prefixes (same as discover)"
    );
    let collapsed = notes.replace('\n', " ");
    assert!(
        !collapsed.contains("`watch_dirs` is unchanged"),
        "BLINE_CONSUMER must not say watch_dirs is unchanged after ignore prefixes are omitted"
    );
    assert!(
        notes.contains("Empty or whitespace-only items are ignored")
            && notes.contains("Omitted MCP `ignore`")
            && notes.contains("Present `null` is a type error"),
        "BLINE_CONSUMER must keep ignore empty-item / omitted / null rules when merging the leftover paragraph"
    );
}

/// Host notes must name the shared empty-trigger policy from #257.
/// Stale "only disable_model_invocation" sent a Bline integrator the
/// wrong auto-inject rule after compat vendors stopped always-active.
#[test]
fn bline_consumer_names_empty_trigger_policy() {
    let notes = repo_file("factory/BLINE_CONSUMER.md");
    assert!(
        notes.contains("empty_triggers_not_always_active"),
        "BLINE_CONSUMER must name SkillSource::empty_triggers_not_always_active"
    );
    assert!(
        notes.contains("filter_skills") && notes.contains("why.activation"),
        "BLINE_CONSUMER must say filter_skills and why.activation share the empty-trigger rule"
    );
    assert!(
        notes.contains("COMPAT_VENDOR_TOKENS")
            && notes.contains("claude")
            && notes.contains("cursor")
            && notes.contains("grok"),
        "BLINE_CONSUMER must name compat vendors whose empty triggers are not always-active"
    );
    assert!(
        notes.contains("`bline` vendor") && notes.contains("auto-inject"),
        "BLINE_CONSUMER must say bline vendor empty triggers still auto-inject"
    );
    assert!(
        notes.contains("vendor_empty_triggers"),
        "BLINE_CONSUMER must name the vendor_empty_triggers activation reason"
    );
    let collapsed = notes.replace('\n', " ");
    assert!(
        !collapsed.contains("still use only")
            || !collapsed.contains("disable_model_invocation for auto-inject"),
        "BLINE_CONSUMER must not say filter_skills still uses only disable_model_invocation"
    );
}

/// Catalog stays cheap (name + description, plus `Use when:`). Watch is
/// paths. Host notes must not group `list --catalog` / MCP catalog or
/// watch with SkillSummary JSON keys (`user_invocable`, …).
#[test]
fn bline_consumer_does_not_group_catalog_with_skill_summary_json() {
    let notes = repo_file("factory/BLINE_CONSUMER.md");
    for sentence in notes.split('.') {
        let s = sentence.replace('\n', " ");
        let groups_cheap_wire = s.contains("`list --catalog`")
            || s.contains("catalog, or watch")
            || (s.contains("catalog") && s.contains("watch") && s.contains("include"));
        let claims_summary = s.contains("user_invocable")
            || s.contains("disable_model_invocation")
            || s.contains("allowed_tools");
        assert!(
            !groups_cheap_wire || !claims_summary,
            "BLINE_CONSUMER must not group catalog/watch with SkillSummary JSON fields: {s}"
        );
    }
}
