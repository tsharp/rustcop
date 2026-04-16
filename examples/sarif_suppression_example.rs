use std::{path::Path, path::PathBuf};

use rustcop::{
    diagnostic::{Diagnostic, Severity},
    output::write_output,
};

fn main() {
    // Example diagnostics with suppression information
    let diagnostics = vec![
        Diagnostic {
            rule_id: "RC1001".to_string(),
            message: "Import formatting issue".to_string(),
            file: PathBuf::from("src/main.rs"),
            line: 10,
            severity: Severity::Warning,
            suppressed: false,
            suppression_justification: None,
        },
        Diagnostic {
            rule_id: "RC1002".to_string(),
            message: "Code style issue".to_string(),
            file: PathBuf::from("src/lib.rs"),
            line: 42,
            severity: Severity::Warning,
            suppressed: true,
            suppression_justification: Some("Performance-critical section".to_string()),
        },
        Diagnostic {
            rule_id: "RC1003".to_string(),
            message: "Another issue".to_string(),
            file: PathBuf::from("src/lib.rs"),
            line: 100,
            severity: Severity::Error,
            suppressed: true,
            suppression_justification: Some("Legacy API compatibility".to_string()),
        },
    ];

    // Write SARIF output
    if write_output(
        Path::new("example_with_suppressions.sarif"),
        &diagnostics,
        rustcop::output::OutputFormat::Sarif,
    )
    .is_ok()
    {
        println!("SARIF output written to example_with_suppressions.sarif");
        println!("\nThis SARIF file includes:");
        println!("- Regular diagnostic (RC1001) - not suppressed");
        println!("- Suppressed warnings/errors (RC1002, RC1003) with justifications");
        println!("\nSuppressed diagnostics are marked with:");
        println!("  \"suppressions\": [{{");
        println!("    \"kind\": \"inSource\",");
        println!("    \"justification\": \"<reason>\"");
        println!("  }}]");
    }

    // Write JSON output
    if write_output(
        Path::new("example_with_suppressions.json"),
        &diagnostics,
        rustcop::output::OutputFormat::Json,
    )
    .is_ok()
    {
        println!("\nJSON output written to example_with_suppressions.json");
    }
}
