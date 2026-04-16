use std::{path::PathBuf, process};

use clap::{Parser, Subcommand};
use colored::Colorize;

use config::Config;
use diagnostic::{Diagnostic, Severity};
use files::discover_files;
use output::{write_output, OutputFormat};
use rules::{
    imports::ImportFormattingRule, modules::ModulesRule, super_imports::DisallowSuperImportsRule,
    wildcard_imports::DisallowWildcardImportsRule, Rule,
};
use suppression::SuppressionParser;

pub mod config;
pub mod diagnostic;
pub mod files;
pub mod output;
pub mod rules;
pub mod suppression;

// Re-export the procedural macros for users
// rustcop::ignore RC2002: This is expected to be a wildcard export.
pub use rustcop_macros::*;

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
    let rules: Vec<Box<dyn Rule>> = vec![
        Box::new(ImportFormattingRule::from_config(&config)),
        Box::new(ModulesRule::from_config(&config)),
        Box::new(DisallowSuperImportsRule::from_config(&config)),
        Box::new(DisallowWildcardImportsRule::from_config(&config)),
    ];

    if rules.is_empty() {
        println!("No rules enabled – nothing to do.");
        process::exit(0);
    }

    let mut total_diagnostics = 0usize;
    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;
    let mut files_fixed = 0usize;
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();

    let treat_warnings_as_errors = config.treat_warnings_as_errors();

    for file in &files {
        let mut content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{} {}: {e}", "error:".red(), file.display());
                continue;
            }
        };

        // Parse suppressions from the file
        let mut suppression_parser = SuppressionParser::parse(&content);

        let mut file_changed = false;
        let mut file_diagnostics = Vec::new();

        for rule in &rules {
            let mut diagnostics = rule.check(&content, file);

            // Check each diagnostic against suppressions
            for diagnostic in &mut diagnostics {
                let (is_suppressed, justification) =
                    suppression_parser.is_suppressed(diagnostic.line, &diagnostic.rule_id);
                if is_suppressed {
                    diagnostic.suppressed = true;
                    diagnostic.suppression_justification = justification;
                }
            }

            // Count diagnostics (suppressed diagnostics still count for stats but not for printing)
            let unsuppressed_count = diagnostics.iter().filter(|d| !d.suppressed).count();
            total_diagnostics += unsuppressed_count;
            total_errors += diagnostics
                .iter()
                .filter(|d| !d.suppressed && matches!(d.severity, Severity::Error))
                .count();
            total_warnings += diagnostics
                .iter()
                .filter(|d| !d.suppressed && matches!(d.severity, Severity::Warning))
                .count();

            // Collect for structured output (include all diagnostics even suppressed ones)
            all_diagnostics.extend(diagnostics.clone());
            file_diagnostics.extend(diagnostics.clone());

            // Only process unsuppressed diagnostics for fix/display
            let unsuppressed_diagnostics: Vec<_> = diagnostics
                .iter()
                .filter(|d| !d.suppressed)
                .cloned()
                .collect();

            if is_fix && !unsuppressed_diagnostics.is_empty() {
                let fixed = rule.fix(&content);
                if fixed != content {
                    content = fixed;
                    file_changed = true;
                    for d in &unsuppressed_diagnostics {
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
                    for d in &unsuppressed_diagnostics {
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
                for d in &unsuppressed_diagnostics {
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

        // After checking all rules, look for unused suppressions
        let unused_suppressions = suppression_parser.get_unused_suppressions();
        for unused in unused_suppressions {
            let diagnostic = Diagnostic {
                rule_id: "RC9001".to_string(),
                message: format!("Unused suppression: {}", unused.description),
                file: file.clone(),
                line: unused.directive_line,
                severity: Severity::Error,
                suppressed: false,
                suppression_justification: None,
            };

            // Allow suppressing generated suppression diagnostics as well.
            let (is_suppressed, _) =
                suppression_parser.is_suppressed(diagnostic.line, &diagnostic.rule_id);
            if is_suppressed {
                continue;
            }

            // Print the error
            println!(
                "{} {} [{}]: {}",
                format!("{}:{}", file.display(), diagnostic.line).bold(),
                "error".red(),
                diagnostic.rule_id.dimmed(),
                diagnostic.message,
            );

            total_diagnostics += 1;
            total_errors += 1;
            all_diagnostics.push(diagnostic);
        }

        // Check for suppressions without justification (if required by config)
        if config.require_suppression_justification() {
            let suppressions_without_justification: Vec<(usize, String)> = suppression_parser
                .get_suppressions_without_justification()
                .iter()
                .map(|suppression| match suppression {
                    suppression::Suppression::FileLevel { directive_line, .. } => {
                        (*directive_line, "file-level suppression".to_string())
                    }
                    suppression::Suppression::LineLevel {
                        directive_line,
                        line,
                        ..
                    } => (*directive_line, format!("suppression on line {}", line)),
                    suppression::Suppression::SpecificRule {
                        directive_line,
                        rule,
                        line,
                        ..
                    } => (
                        *directive_line,
                        format!("suppression for rule {} on line {}", rule, line),
                    ),
                })
                .collect();

            for (directive_line, description) in suppressions_without_justification {
                let diagnostic = Diagnostic {
                    rule_id: "RC9002".to_string(),
                    message: format!("Suppression missing justification: {}", description),
                    file: file.clone(),
                    line: directive_line,
                    severity: Severity::Error,
                    suppressed: false,
                    suppression_justification: None,
                };

                let (is_suppressed, _) =
                    suppression_parser.is_suppressed(diagnostic.line, &diagnostic.rule_id);
                if is_suppressed {
                    continue;
                }

                // Print the error
                println!(
                    "{} {} [{}]: {}",
                    format!("{}:{}", file.display(), diagnostic.line).bold(),
                    "error".red(),
                    diagnostic.rule_id.dimmed(),
                    diagnostic.message,
                );

                total_diagnostics += 1;
                total_errors += 1;
                all_diagnostics.push(diagnostic);
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

            let mut suppression_parser = SuppressionParser::parse(&content);

            for rule in &rules {
                let mut diags = rule.check(&content, file);
                for diagnostic in &mut diags {
                    let (is_suppressed, justification) =
                        suppression_parser.is_suppressed(diagnostic.line, &diagnostic.rule_id);
                    if is_suppressed {
                        diagnostic.suppressed = true;
                        diagnostic.suppression_justification = justification;
                    }
                }

                remaining_errors += diags
                    .iter()
                    .filter(|d| !d.suppressed && matches!(d.severity, Severity::Error))
                    .count();
                remaining_warnings += diags
                    .iter()
                    .filter(|d| !d.suppressed && matches!(d.severity, Severity::Warning))
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
            if remaining_errors > 0 || (treat_warnings_as_errors && remaining_warnings > 0) {
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
        // Exit with error code if there are errors, or if warnings-as-errors is enabled
        if total_errors > 0 || (treat_warnings_as_errors && total_warnings > 0) {
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
