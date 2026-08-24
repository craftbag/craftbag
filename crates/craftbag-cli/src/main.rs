//! craftbag CLI: list, load, why, validate.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use craftbag::{
    DiscoveryOptions, FormatOptions, Skill, discover, find_skill_by_name, format_load_message,
    progressive_budgets, unknown_or_skipped_skill_message, validate_path, why,
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
        #[arg(long)]
        json: bool,
        #[arg(long = "path", value_name = "PATH")]
        paths: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        vendor: Vec<String>,
        #[arg(long = "user-dir")]
        user_dir: Option<PathBuf>,
    },
    /// Print one skill body plus package envelope.
    Load {
        name: String,
        #[arg(long = "args", default_value = "")]
        args: String,
        #[arg(long = "path", value_name = "PATH")]
        paths: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        vendor: Vec<String>,
        #[arg(long = "user-dir")]
        user_dir: Option<PathBuf>,
    },
    /// Explain loaded, skipped, and activation decisions.
    Why {
        name: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        context: Option<String>,
        #[arg(long, default_value_t = 8_000)]
        context_tokens: usize,
        #[arg(long = "path", value_name = "PATH")]
        paths: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        vendor: Vec<String>,
        #[arg(long = "user-dir")]
        user_dir: Option<PathBuf>,
    },
    /// Validate one SKILL.md path.
    Validate { path: PathBuf },
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
            paths,
            vendor,
            user_dir,
        } => {
            let report = discover_cwd(&paths, &vendor, user_dir)?;
            if json {
                let v = serde_json::json!({
                    "skills": report.skills.iter().map(skill_json).collect::<Vec<_>>(),
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
        } => {
            let report = discover_cwd(&paths, &vendor, user_dir)?;
            match find_skill_by_name(&report.skills, &name) {
                Some(skill) => {
                    print!(
                        "{}",
                        format_load_message(skill, &args, FormatOptions::default())
                    );
                    Ok(ExitCode::SUCCESS)
                }
                None => {
                    let _ = writeln!(
                        io::stderr(),
                        "{}",
                        unknown_or_skipped_skill_message(&name, &report.skips)
                    );
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
        } => {
            let report = discover_cwd(&paths, &vendor, user_dir)?;
            let budgets = progressive_budgets(context_tokens);
            let why = why(&report, name.as_deref(), context.as_deref(), Some(budgets));
            if let Some(want) = name.as_deref() {
                let known = !why.loaded.is_empty() || !why.skips.is_empty();
                if !known {
                    let _ = writeln!(io::stderr(), "unknown skill: {want}");
                    return Ok(ExitCode::from(1));
                }
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
        Cmd::Validate { path } => {
            let report = validate_path(&path);
            if report.ok {
                println!("ok\t{}", report.name.as_deref().unwrap_or("-"));
                Ok(ExitCode::SUCCESS)
            } else {
                for e in &report.errors {
                    let _ = writeln!(io::stderr(), "{e}");
                }
                Ok(ExitCode::from(1))
            }
        }
    }
}

fn discover_cwd(
    paths: &[String],
    vendor: &[String],
    user_dir: Option<PathBuf>,
) -> Result<craftbag::DiscoveryReport, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let opts = DiscoveryOptions {
        paths: paths.to_vec(),
        vendor_roots: vendor.to_vec(),
        user_skills_dir: user_dir,
        ..DiscoveryOptions::default()
    };
    discover(&cwd, &opts).map_err(|e| e.to_string())
}

fn skill_json(skill: &Skill) -> serde_json::Value {
    serde_json::json!({
        "name": skill.name,
        "description": skill.description,
        "source": skill.source,
        "path": skill.source_path,
    })
}
