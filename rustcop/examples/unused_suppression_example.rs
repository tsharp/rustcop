use std::path::PathBuf;

use rustcop::{
    diagnostic::{Diagnostic, Severity},
    suppression::SuppressionParser,
};

fn main() {
    // Simulate source code with some used and unused suppressions
    let source = r#"
// rustcop:ignore RC1001: This one will be used
use std::fs;

// rustcop:ignore RC2001: This one is unused
fn foo() {}

// rustcop:ignore RC3001: Another unused one
const X: i32 = 42;
"#;

    // Parse suppressions
    let mut parser = SuppressionParser::parse(source);

    // Simulate some diagnostics
    let mut diagnostics = vec![
        Diagnostic {
            rule_id: "RC1001".to_string(),
            message: "Import issue".to_string(),
            file: PathBuf::from("example.rs"),
            line: 3, // This will match the suppression on line 2
            severity: Severity::Warning,
            suppressed: false,
            suppression_justification: None,
        },
        Diagnostic {
            rule_id: "RC4001".to_string(),
            message: "Some other issue".to_string(),
            file: PathBuf::from("example.rs"),
            line: 10,
            severity: Severity::Error,
            suppressed: false,
            suppression_justification: None,
        },
    ];

    // Check each diagnostic against suppressions (this marks suppressions as used)
    for diagnostic in &mut diagnostics {
        let (is_suppressed, justification) =
            parser.is_suppressed(diagnostic.line, &diagnostic.rule_id);
        if is_suppressed {
            diagnostic.suppressed = true;
            diagnostic.suppression_justification = justification;
        }
    }

    println!("=== DIAGNOSTICS ===");
    for d in &diagnostics {
        if d.suppressed {
            println!(
                "  ✓ {} at line {} - SUPPRESSED ({})",
                d.rule_id,
                d.line,
                d.suppression_justification.as_deref().unwrap_or("no reason")
            );
        } else {
            println!("  ✗ {} at line {} - ACTIVE", d.rule_id, d.line);
        }
    }

    // Check for unused suppressions
    let unused = parser.get_unused_suppressions();

    println!("\n=== UNUSED SUPPRESSIONS ===");
    if unused.is_empty() {
        println!("  (none)");
    } else {
        for u in &unused {
            println!(
                "  ! Line {}: Suppression for {} was never used",
                u.directive_line, u.description
            );
        }
    }

    // Generate error diagnostics for unused suppressions
    println!("\n=== GENERATED ERRORS FOR UNUSED SUPPRESSIONS ===");
    for u in &unused {
        let diagnostic = Diagnostic {
            rule_id: "RC9001".to_string(),
            message: format!("Unused suppression: {}", u.description),
            file: PathBuf::from("example.rs"),
            line: u.directive_line,
            severity: Severity::Error,
            suppressed: false,
            suppression_justification: None,
        };

        println!(
            "  example.rs:{} error [{}]: {}",
            diagnostic.line, diagnostic.rule_id, diagnostic.message
        );
    }
}
