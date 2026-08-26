//! Local rustc must match CI. A newer default toolchain can accept
//! let-chains that fail the 1.85 job.

fn repo_file(rel: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn rust_toolchain_toml_matches_cargo_msrv_and_ci() {
    let toolchain = repo_file("rust-toolchain.toml");
    assert!(
        toolchain.contains("channel = \"1.85\""),
        "rust-toolchain.toml must pin 1.85: {toolchain}"
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

/// Local AGENTS.md gate and hosted CI must run the same commands.
#[test]
fn local_gate_commands_run_in_ci() {
    let agents = repo_file("AGENTS.md");
    let ci = repo_file(".github/workflows/ci.yml");
    assert!(
        ci.contains("- 'AGENTS.md'"),
        "rust path-filter must include AGENTS.md so a local-gate edit runs this test"
    );
    let commands = [
        "cargo fmt --check",
        "cargo clippy --locked --workspace --all-targets -- -D warnings",
        "cargo nextest run --locked --workspace",
        "cargo test --locked --workspace --doc",
        "bash factory/scripts/deny-check.sh",
        "bash factory/scripts/assert-stealth.sh craftbag/craftbag",
        "bash factory/scripts/write-ledger.sh --self-test",
    ];
    for cmd in commands {
        assert!(agents.contains(cmd), "AGENTS.md local gate must list {cmd}");
        assert!(
            ci.contains(cmd),
            "ci.yml must run {cmd}; a local-only gate command is not CI"
        );
    }
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
