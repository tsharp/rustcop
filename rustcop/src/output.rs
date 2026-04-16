use std::path::Path;

use serde::Serialize;
use serde_sarif::sarif::{
    ArtifactLocation, Location, Message, PhysicalLocation, Region, ReportingDescriptor,
    Result as SarifResult, Run, Sarif, Suppression as SarifSuppression, Tool, ToolComponent,
};

use crate::diagnostic::{Diagnostic, Severity};

/// Output format detected from file extension
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Sarif,
    Json,
}

impl OutputFormat {
    /// Detect format from file extension
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| match ext.to_lowercase().as_str() {
                "sarif" => Some(Self::Sarif),
                "json" => Some(Self::Json),
                _ => None,
            })
    }
}

/// Write diagnostics to a file in the specified format
pub fn write_output(
    path: &Path,
    diagnostics: &[Diagnostic],
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = match format {
        OutputFormat::Sarif => generate_sarif(diagnostics)?,
        OutputFormat::Json => generate_json(diagnostics)?,
    };

    std::fs::write(path, content)?;
    Ok(())
}

/// Generate SARIF JSON (Static Analysis Results Interchange Format)
/// Using the serde-sarif crate for spec compliance
fn generate_sarif(diagnostics: &[Diagnostic]) -> Result<String, Box<dyn std::error::Error>> {
    // Create tool descriptor
    let driver = ToolComponent::builder()
        .name("rustcop")
        .version(env!("CARGO_PKG_VERSION"))
        .information_uri("https://github.com/tsharp/rustcop")
        .rules(generate_sarif_rules(diagnostics))
        .build();

    let tool = Tool::builder().driver(driver).build();

    // Convert diagnostics to SARIF results
    let results: Vec<SarifResult> = diagnostics
        .iter()
        .map(|d| {
            let artifact_location = ArtifactLocation::builder()
                .uri(d.file.display().to_string())
                .build();

            let region = Region::builder().start_line(d.line as i64).build();

            let physical_location = PhysicalLocation::builder()
                .artifact_location(artifact_location)
                .region(region)
                .build();

            let location = Location::builder()
                .physical_location(physical_location)
                .build();

            let message = Message::builder().text(&d.message).build();

            // Build result with or without suppression
            if d.suppressed {
                let suppression = if let Some(justification) = &d.suppression_justification {
                    SarifSuppression::builder()
                        .kind("inSource")
                        .justification(justification)
                        .build()
                } else {
                    SarifSuppression::builder().kind("inSource").build()
                };

                SarifResult::builder()
                    .rule_id(&d.rule_id)
                    .level(match d.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                    })
                    .message(message)
                    .locations(vec![location])
                    .suppressions(vec![suppression])
                    .build()
            } else {
                SarifResult::builder()
                    .rule_id(&d.rule_id)
                    .level(match d.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                    })
                    .message(message)
                    .locations(vec![location])
                    .build()
            }
        })
        .collect();

    let run = Run::builder().tool(tool).results(results).build();

    let sarif = Sarif::builder().version("2.1.0").runs(vec![run]).build();

    Ok(serde_json::to_string_pretty(&sarif)?)
}

/// Generate unique rules from diagnostics for SARIF
fn generate_sarif_rules(diagnostics: &[Diagnostic]) -> Vec<ReportingDescriptor> {
    let mut seen_rules = std::collections::HashSet::new();
    let mut rules = Vec::new();

    for diag in diagnostics {
        if seen_rules.insert(&diag.rule_id) {
            let rule = ReportingDescriptor::builder()
                .id(&diag.rule_id)
                .short_description(&diag.message)
                .build();
            rules.push(rule);
        }
    }

    rules
}

/// Generate simple JSON output
fn generate_json(diagnostics: &[Diagnostic]) -> Result<String, Box<dyn std::error::Error>> {
    #[derive(Serialize)]
    struct JsonDiagnostic<'a> {
        rule_id: &'a str,
        severity: &'static str,
        message: &'a str,
        file: String,
        line: usize,
        suppressed: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        suppression_justification: Option<&'a str>,
    }

    let json_diagnostics: Vec<_> = diagnostics
        .iter()
        .map(|d| JsonDiagnostic {
            rule_id: &d.rule_id,
            severity: match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            message: &d.message,
            file: d.file.display().to_string(),
            line: d.line,
            suppressed: d.suppressed,
            suppression_justification: d.suppression_justification.as_deref(),
        })
        .collect();

    Ok(serde_json::to_string_pretty(&json_diagnostics)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_format_detection() {
        assert_eq!(
            OutputFormat::from_path(Path::new("output.sarif")),
            Some(OutputFormat::Sarif)
        );
        assert_eq!(
            OutputFormat::from_path(Path::new("output.json")),
            Some(OutputFormat::Json)
        );
        assert_eq!(OutputFormat::from_path(Path::new("output.txt")), None);
    }

    #[test]
    fn test_json_output() {
        let diagnostics = vec![Diagnostic {
            rule_id: "RC1001".to_string(),
            message: "Test message".to_string(),
            file: PathBuf::from("test.rs"),
            line: 42,
            severity: Severity::Warning,            suppressed: false,
            suppression_justification: None,        }];

        let json = generate_json(&diagnostics).unwrap();
        assert!(json.contains("RC1001"));
        assert!(json.contains("Test message"));
        assert!(json.contains("warning"));
        assert!(json.contains("test.rs"));
    }

    #[test]
    fn test_sarif_output() {
        let diagnostics = vec![Diagnostic {
            rule_id: "RC1001".to_string(),
            message: "Test message".to_string(),
            file: PathBuf::from("test.rs"),
            line: 42,
            severity: Severity::Error,
            suppressed: false,
            suppression_justification: None,
        }];

        let sarif = generate_sarif(&diagnostics).unwrap();
        assert!(sarif.contains("2.1.0"));
        assert!(sarif.contains("rustcop"));
        assert!(sarif.contains("RC1001"));
        assert!(sarif.contains("error"));
    }

    #[test]
    fn test_sarif_output_with_suppression() {
        let diagnostics = vec![
            Diagnostic {
                rule_id: "RC1001".to_string(),
                message: "Normal diagnostic".to_string(),
                file: PathBuf::from("test.rs"),
                line: 10,
                severity: Severity::Warning,
                suppressed: false,
                suppression_justification: None,
            },
            Diagnostic {
                rule_id: "RC1002".to_string(),
                message: "Suppressed diagnostic".to_string(),
                file: PathBuf::from("test.rs"),
                line: 20,
                severity: Severity::Warning,
                suppressed: true,
                suppression_justification: Some("Performance optimization".to_string()),
            },
        ];

        let sarif = generate_sarif(&diagnostics).unwrap();
        
        // Should contain both diagnostics
        assert!(sarif.contains("RC1001"));
        assert!(sarif.contains("RC1002"));
        
        // Should contain suppression info
        assert!(sarif.contains("suppressions"));
        assert!(sarif.contains("inSource"));
        assert!(sarif.contains("Performance optimization"));
    }

    #[test]
    fn test_json_output_with_suppression() {
        let diagnostics = vec![
            Diagnostic {
                rule_id: "RC1001".to_string(),
                message: "Suppressed with justification".to_string(),
                file: PathBuf::from("test.rs"),
                line: 10,
                severity: Severity::Warning,
                suppressed: true,
                suppression_justification: Some("Legacy API".to_string()),
            },
        ];

        let json = generate_json(&diagnostics).unwrap();
        assert!(json.contains("RC1001"));
        assert!(json.contains("\"suppressed\": true"));
        assert!(json.contains("Legacy API"));
    }
}
