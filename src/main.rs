use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use colored::Colorize;

use rustcop::config::Config;
use rustcop::diagnostic::Severity;
use rustcop::files::discover_files;
use rustcop::rules::imports::ImportFormattingRule;
use rustcop::rules::Rule;

#[derive(Parser)]
#[command(
    name = "rustcop",
    version,
    about = "A Rust style linter and formatter inspired by C#'s StyleCop"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to the rustcop config file
    #[arg(short, long, default_value = "rustcop.toml")]
    config: PathBuf,
}

#[derive(Subcommand)]
enum Commands {
    /// Check files for style violations without modifying them
    Check {
        /// Files or directories to check
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
    },
    /// Automatically fix style violations
    Fix {
        /// Files or directories to fix
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    let config = Config::load(&cli.config).unwrap_or_else(|e| {
        eprintln!(
            "{} Could not load config ({}): using defaults.",
            "warning:".yellow(),
            e
        );
        Config::default()
    });

    // Build the set of enabled rules
    let mut rules: Vec<Box<dyn Rule>> = Vec::new();
    if config.imports.enabled {
        rules.push(Box::new(ImportFormattingRule::from_config(&config)));
    }

    if rules.is_empty() {
        println!("No rules enabled – nothing to do.");
        return;
    }

    let (paths, is_fix) = match &cli.command {
        Commands::Check { paths } => (paths.clone(), false),
        Commands::Fix { paths } => (paths.clone(), true),
    };

    let files = discover_files(&paths);

    if files.is_empty() {
        println!("No .rs files found.");
        return;
    }

    let mut total_diagnostics = 0usize;
    let mut files_fixed = 0usize;

    for file in &files {
        let mut content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} {}: {e}", "error:".red(), file.display());
                continue;
            }
        };

        let mut file_changed = false;

        for rule in &rules {
            let diagnostics = rule.check(&content, file);
            total_diagnostics += diagnostics.len();

            for d in &diagnostics {
                let sev = match d.severity {
                    Severity::Warning => "warning".yellow(),
                    Severity::Error => "error".red(),
                };
                println!(
                    "{} {} [{}]: {}",
                    format!("{}:{}", file.display(), d.line).bold(),
                    sev,
                    d.rule_id.dimmed(),
                    d.message,
                );
            }

            if is_fix && !diagnostics.is_empty() {
                let fixed = rule.fix(&content);
                if fixed != content {
                    content = fixed;
                    file_changed = true;
                }
            }
        }

        if file_changed {
            if let Err(e) = std::fs::write(file, &content) {
                eprintln!("{} could not write {}: {e}", "error:".red(), file.display());
            } else {
                files_fixed += 1;
            }
        }
    }

    println!();
    if is_fix {
        println!(
            "{} file(s) checked, {} fixed.",
            files.len(),
            files_fixed
        );
    } else if total_diagnostics > 0 {
        println!(
            "{} diagnostic(s) in {} file(s).",
            total_diagnostics,
            files.len()
        );
        process::exit(1);
    } else {
        println!("{}", "All checks passed!".green());
    }
}
