//! craftbag CLI: list, load, why, validate.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use craftbag::{
    DiscoveryOptions, FormatOptions, ListFormat, SkillSource, SkillSummary, discover,
    find_skill_by_name, format_available_skills_xml, format_catalog, format_load_message,
    parse_list_format, progressive_budgets, unknown_or_skipped_skill, validate_path_with_options,
    watch_dirs, why,
};

#[derive(Parser)]
#[command(name = "craftbag", version, about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Catalog discovered skills.
    List {
        /// Print `{ skills, skips }` (same shape as MCP skills_list).
        #[arg(long)]
        json: bool,
        /// Official skills-ref `<available_skills>` XML for host prompts.
        #[arg(long, conflicts_with = "json")]
        xml: bool,
        /// Markdown catalog (name + description) for host prompts.
        #[arg(long, conflicts_with_all = ["json", "xml"])]
        catalog: bool,
        /// Print notify-watch roots (same walk as discover). Does not load SKILL.md.
        #[arg(long = "watch-dirs", conflicts_with_all = ["json", "xml", "catalog"])]
        watch_dirs: bool,
        /// Same tokens as MCP skills_list format: json (`{ skills, skips }`), xml (`<available_skills>`), catalog, watch.
        /// `watch-dirs` and `watch_dirs` are the `--watch-dirs` flag name.
        #[arg(long = "format", value_name = "FORMAT", conflicts_with_all = ["json", "xml", "catalog", "watch_dirs"])]
        format: Option<String>,
        /// Extra package or collection root (not a project walk). Example: --path ./my-skill
        #[arg(long = "path", value_name = "PATH")]
        paths: Vec<String>,
        /// Opt-in vendor trees: bline, claude, cursor, grok. Example: --vendor claude
        #[arg(long, value_delimiter = ',')]
        vendor: Vec<String>,
        /// User skills root (child dirs are packages). Example: --user-dir ~/myskills
        #[arg(long = "user-dir")]
        user_dir: Option<PathBuf>,
        /// Reject names outside `a-z0-9-`. Default still allows Unicode / NFKC.
        #[arg(long = "ascii-names")]
        ascii_names: bool,
    },
    /// Print one skill body plus package envelope (includes argument-hint, when-to-use, and allowed-tools).
    Load {
        name: String,
        /// Copied into the envelope as User arguments. Matches argument-hint. Example: --args --fix
        #[arg(long = "args", default_value = "")]
        args: String,
        /// Extra package or collection root (not a project walk). Example: --path ./my-skill
        #[arg(long = "path", value_name = "PATH")]
        paths: Vec<String>,
        /// Opt-in vendor trees: bline, claude, cursor, grok. Example: --vendor claude
        #[arg(long, value_delimiter = ',')]
        vendor: Vec<String>,
        /// User skills root (child dirs are packages). Example: --user-dir ~/myskills
        #[arg(long = "user-dir")]
        user_dir: Option<PathBuf>,
        /// Reject names outside `a-z0-9-`. Default still allows Unicode / NFKC.
        #[arg(long = "ascii-names")]
        ascii_names: bool,
    },
    /// Explain loaded, skipped, and activation decisions.
    Why {
        name: Option<String>,
        /// Print `{ error_kind, error }` on a miss (same peel as `validate --json`).
        #[arg(long)]
        json: bool,
        /// Activation context text. Example: --context rebase
        #[arg(long)]
        context: Option<String>,
        /// Model context window size (default 8000).
        #[arg(long, default_value_t = 8_000)]
        context_tokens: usize,
        /// Extra package or collection root (not a project walk). Example: --path ./my-skill
        #[arg(long = "path", value_name = "PATH")]
        paths: Vec<String>,
        /// Opt-in vendor trees: bline, claude, cursor, grok. Example: --vendor claude
        #[arg(long, value_delimiter = ',')]
        vendor: Vec<String>,
        /// User skills root (child dirs are packages). Example: --user-dir ~/myskills
        #[arg(long = "user-dir")]
        user_dir: Option<PathBuf>,
        /// Reject names outside `a-z0-9-`. Default still allows Unicode / NFKC.
        #[arg(long = "ascii-names")]
        ascii_names: bool,
    },
    /// Validate one SKILL.md path.
    Validate {
        path: PathBuf,
        /// Reject unknown frontmatter keys (skills-ref default).
        #[arg(long)]
        strict: bool,
        /// Same `{ error_kind, error }` peel as `why --json` on failure, plus `path`.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(io::stderr(), "{e}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.cmd {
        Cmd::List {
            json,
            xml,
            catalog,
            watch_dirs: watch,
            format,
            paths,
            vendor,
            user_dir,
            ascii_names,
        } => {
            let mode = list_output_mode(json, xml, catalog, watch, format)?;
            if matches!(mode, ListOutput::Watch) {
                let (cwd, opts) = discovery_opts(&paths, &vendor, user_dir, ascii_names)?;
                for dir in watch_dirs(&cwd, &opts) {
                    println!("{}", dir.display());
                }
                return Ok(ExitCode::SUCCESS);
            }
            let report = discover_cwd(&paths, &vendor, user_dir, ascii_names)?;
            if matches!(mode, ListOutput::Catalog) {
                print!(
                    "{}",
                    format_catalog(
                        &report.skills,
                        "",
                        progressive_budgets(8_000),
                        FormatOptions::default(),
                    )
                );
            } else if matches!(mode, ListOutput::Xml) {
                print!("{}", format_available_skills_xml(&report.skills));
            } else if matches!(mode, ListOutput::Json) {
                let v = serde_json::json!({
                    "skills": report.skills.iter().map(SkillSummary::from).collect::<Vec<_>>(),
                    "skips": report.skips,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
                );
            } else {
                for s in &report.skills {
                    println!(
                        "{}\t{}\t{}",
                        s.name,
                        s.source.as_str(),
                        s.source_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    );
                }
                for skip in &report.skips {
                    let _ = writeln!(
                        io::stderr(),
                        "skip\t{}\t{}\t{}",
                        skip.kind.as_str(),
                        skip.path.display(),
                        skip.detail
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Load {
            name,
            args,
            paths,
            vendor,
            user_dir,
            ascii_names,
        } => {
            let report = discover_cwd(&paths, &vendor, user_dir, ascii_names)?;
            match find_skill_by_name(&report.skills, &name) {
                Some(skill) => {
                    print!(
                        "{}",
                        format_load_message(skill, &args, FormatOptions::default())
                    );
                    Ok(ExitCode::SUCCESS)
                }
                None => {
                    let miss = unknown_or_skipped_skill(&name, &report.skips);
                    let _ = writeln!(io::stderr(), "{miss}");
                    Ok(ExitCode::from(2))
                }
            }
        }
        Cmd::Why {
            name,
            json,
            context,
            context_tokens,
            paths,
            vendor,
            user_dir,
            ascii_names,
        } => {
            let report = discover_cwd(&paths, &vendor, user_dir, ascii_names)?;
            let budgets = progressive_budgets(context_tokens);
            let why = why(&report, name.as_deref(), context.as_deref(), Some(budgets));
            if let Some(miss) = why.unknown_skill_miss() {
                let _ = writeln!(io::stderr(), "{miss}");
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&miss).map_err(|e| e.to_string())?
                    );
                }
                return Ok(ExitCode::from(1));
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&why).map_err(|e| e.to_string())?
                );
            } else {
                for s in &why.loaded {
                    println!(
                        "loaded\t{}\t{}",
                        s.name,
                        s.path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    );
                }
                for skip in &why.skips {
                    println!(
                        "skip\t{}\t{}\t{}",
                        skip.kind.as_str(),
                        skip.path.display(),
                        skip.detail
                    );
                }
                for a in &why.activation {
                    println!(
                        "activation\t{}\t{}\t{}",
                        a.name,
                        a.reason.as_str(),
                        a.detail
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Validate { path, strict, json } => {
            let report = validate_path_with_options(&path, strict);
            if report.ok {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                    );
                } else {
                    println!("ok\t{}", report.name.as_deref().unwrap_or("-"));
                }
                Ok(ExitCode::SUCCESS)
            } else if let (true, Some(miss)) = (json, report.miss()) {
                let _ = writeln!(io::stderr(), "{miss}");
                println!(
                    "{}",
                    serde_json::to_string_pretty(&miss).map_err(|e| e.to_string())?
                );
                Ok(ExitCode::from(1))
            } else {
                for e in &report.errors {
                    let _ = writeln!(io::stderr(), "{e}");
                }
                Ok(ExitCode::from(1))
            }
        }
    }
}

enum ListOutput {
    Tsv,
    Json,
    Xml,
    Catalog,
    Watch,
}

fn list_output_mode(
    json: bool,
    xml: bool,
    catalog: bool,
    watch: bool,
    format: Option<String>,
) -> Result<ListOutput, String> {
    if let Some(format) = format {
        return Ok(match parse_list_format(&format)? {
            ListFormat::Json => ListOutput::Json,
            ListFormat::Xml => ListOutput::Xml,
            ListFormat::Catalog => ListOutput::Catalog,
            ListFormat::Watch => ListOutput::Watch,
        });
    }
    if watch {
        return Ok(ListOutput::Watch);
    }
    if catalog {
        return Ok(ListOutput::Catalog);
    }
    if xml {
        return Ok(ListOutput::Xml);
    }
    if json {
        return Ok(ListOutput::Json);
    }
    Ok(ListOutput::Tsv)
}

fn discovery_opts(
    paths: &[String],
    vendor: &[String],
    user_dir: Option<PathBuf>,
    ascii_names: bool,
) -> Result<(PathBuf, DiscoveryOptions), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let vendor_roots = SkillSource::parse_vendor_roots(vendor)?;
    let opts = DiscoveryOptions {
        paths: paths.to_vec(),
        vendor_roots,
        user_skills_dir: user_dir,
        ascii_names,
        ..DiscoveryOptions::default()
    };
    Ok((cwd, opts))
}

fn discover_cwd(
    paths: &[String],
    vendor: &[String],
    user_dir: Option<PathBuf>,
    ascii_names: bool,
) -> Result<craftbag::DiscoveryReport, String> {
    let (cwd, opts) = discovery_opts(paths, vendor, user_dir, ascii_names)?;
    discover(&cwd, &opts).map_err(|e| e.to_string())
}
