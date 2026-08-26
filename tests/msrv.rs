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
