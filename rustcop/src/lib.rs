use std::{path::PathBuf, process};

use clap::{Parser, Subcommand};
use colored::Colorize;

pub mod config;
pub mod diagnostic;
pub mod files;
pub mod output;
pub mod rules;
pub mod suppression;

// Re-export the procedural macros for users
pub use rustcop_macros::*;

use config::Config;
use diagnostic::{Diagnostic, Severity};
use files::discover_files;
use output::{write_output, OutputFormat};
use rules::imports::ImportFormattingRule;
use rules::Rule;

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
    #[arg(short, long, default_value = "rustcop.toml", global = true)]
    config: PathBuf,

    /// Write output to file (format inferred from extension: .sarif, .json)
    #[arg(short, long, global = true)]
    out: Option<PathBuf>,
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

/// Run rustcop with the given CLI arguments.
///
/// Pass the arguments as they would appear after the binary name,
/// e.g. `&["check", "--config", "rustcop.toml"]`.
pub fn run<I, T>(args: I) -> !
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);

    let (paths, is_fix) = match &cli.command {
        Commands::Check { paths } => (paths.clone(), false),
        Commands::Fix { paths } => (paths.clone(), true),
    };

    let files = discover_files(&paths);

    if files.is_empty() {
        println!("No .rs files found.");
        process::exit(0);
    }

    // Resolve configuration based on CLI args
    // If a specific config file is provided via --config, load it directly
    // Otherwise, use hierarchical resolution from the first file
    let config = if cli.config.exists() {
        Config::load(&cli.config).unwrap_or_else(|e| {
            eprintln!(
                "{} Could not load config file {}: {}",
                "warning:".yellow(),
                cli.config.display(),
                e
            );
            eprintln!("{} Using built-in defaults.", "info:".blue());
            Config::empty()
        })
    } else if let Some(first_file) = files.first() {
        // Use hierarchical discovery from first file
        Config::resolve_for_file(first_file).unwrap_or_else(|e| {
            eprintln!("{} Could not resolve config: {}", "warning:".yellow(), e);
            eprintln!("{} Using built-in defaults.", "info:".blue());
            Config::empty()
        })
    } else {
        Config::empty()
    };

    // Build the set of enabled rules
    // Import formatting rule is enabled by default
    let rules: Vec<Box<dyn Rule>> = vec![Box::new(ImportFormattingRule::from_config(&config))];

    if rules.is_empty() {
        println!("No rules enabled – nothing to do.");
        process::exit(0);
    }

    let mut total_diagnostics = 0usize;
    let mut total_errors = 0usize;
    let mut files_fixed = 0usize;
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();

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
            total_errors += diagnostics
                .iter()
                .filter(|d| matches!(d.severity, Severity::Error))
                .count();

            // Collect for structured output
            all_diagnostics.extend(diagnostics.clone());

            if is_fix && !diagnostics.is_empty() {
                let fixed = rule.fix(&content);
                if fixed != content {
                    content = fixed;
                    file_changed = true;
                    for d in &diagnostics {
                        println!(
                            "{} {} [{}]: {} {}",
                            format!("{}:{}", file.display(), d.line).bold(),
                            "fixed".green(),
                            d.rule_id.dimmed(),
                            d.message,
                            "(auto-fixed)".green(),
                        );
                    }
                } else {
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
                }
            } else {
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
        println!("{} file(s) checked, {} fixed.", files.len(), files_fixed);

        // Run a final verification pass
        let mut remaining_errors = 0usize;
        let mut remaining_warnings = 0usize;
        for file in &files {
            let content = match std::fs::read_to_string(file) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for rule in &rules {
                let diags = rule.check(&content, file);
                remaining_errors += diags
                    .iter()
                    .filter(|d| matches!(d.severity, Severity::Error))
                    .count();
                remaining_warnings += diags
                    .iter()
                    .filter(|d| matches!(d.severity, Severity::Warning))
                    .count();
            }
        }
        if remaining_errors > 0 || remaining_warnings > 0 {
            println!(
                "{} {} error(s), {} warning(s) remaining after fix.",
                if remaining_errors > 0 {
                    "error:".red()
                } else {
                    "warning:".yellow()
                },
                remaining_errors,
                remaining_warnings
            );
            if remaining_errors > 0 {
                process::exit(1);
            }
        } else {
            println!("{}", "All checks passed after fix!".green());
        }
    } else if total_diagnostics > 0 {
        println!(
            "{} diagnostic(s) in {} file(s).",
            total_diagnostics,
            files.len()
        );
        // Only exit with error code if there are actual errors, not just warnings
        if total_errors > 0 {
            process::exit(1);
        }
    } else {
        println!("{}", "All checks passed!".green());
    }

    // Write structured output if requested
    if let Some(out_path) = &cli.out {
        if let Some(format) = OutputFormat::from_path(out_path) {
            if let Err(e) = write_output(out_path, &all_diagnostics, format) {
                eprintln!(
                    "{} Failed to write output file {}: {}",
                    "error:".red(),
                    out_path.display(),
                    e
                );
                process::exit(1);
            } else {
                eprintln!(
                    "{} Wrote {} to {}",
                    "info:".blue(),
                    match format {
                        OutputFormat::Sarif => "SARIF",
                        OutputFormat::Json => "JSON",
                    },
                    out_path.display()
                );
            }
        } else {
            eprintln!(
                "{} Unsupported output format for {}. Use .sarif or .json",
                "error:".red(),
                out_path.display()
            );
            process::exit(1);
        }
    }

    process::exit(0);
}
