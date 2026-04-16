use std::path::PathBuf;

use rustcop::{
    diagnostic::{Diagnostic, Severity},
    output::{write_output, OutputFormat},
    suppression::SuppressionParser,
};

fn main() {
    let source = r#"
// rustcop:ignore RC1001: Used suppression
use std::fs;

// rustcop:ignore RC2001: Unused suppression
fn foo() {}
"#;

    let mut parser = SuppressionParser::parse(source);

    // Create a diagnostic that will be suppressed
    let mut diagnostics = vec![Diagnostic {
        rule_id: "RC1001".to_string(),
        message: "Import issue".to_string(),
        file: PathBuf::from("test.rs"),
        line: 3,
        severity: Severity::Warning,
        suppressed: false,
        suppression_justification: None,
    }];

    // Check suppressions
    for diagnostic in &mut diagnostics {
        let (is_suppressed, justification) =
            parser.is_suppressed(diagnostic.line, &diagnostic.rule_id);
        if is_suppressed {
            diagnostic.suppressed = true;
            diagnostic.suppression_justification = justification;
        }
    }

    // Add diagnostics for unused suppressions
    for unused in parser.get_unused_suppressions() {
        diagnostics.push(Diagnostic {
            rule_id: "RC9001".to_string(),
            message: format!("Unused suppression: {}", unused.description),
            file: PathBuf::from("test.rs"),
            line: unused.directive_line,
            severity: Severity::Error,
            suppressed: false,
            suppression_justification: None,
        });
    }

    // Write to SARIF
    let sarif_path = PathBuf::from("example_unused_suppressions.sarif");
    write_output(&sarif_path, &diagnostics, OutputFormat::Sarif).unwrap();
    println!("Wrote SARIF to {}", sarif_path.display());

    // Write to JSON
    let json_path = PathBuf::from("example_unused_suppressions.json");
    write_output(&json_path, &diagnostics, OutputFormat::Json).unwrap();
    println!("Wrote JSON to {}", json_path.display());

    println!("\nDiagnostics generated:");
    for d in &diagnostics {
        if d.suppressed {
            println!(
                "  ✓ {}:{} [{}] {} (SUPPRESSED)",
                d.file.display(),
                d.line,
                d.rule_id,
                d.message
            );
        } else {
            println!(
                "  ✗ {}:{} [{}] {}",
                d.file.display(),
                d.line,
                d.rule_id,
                d.message
            );
        }
    }
}
