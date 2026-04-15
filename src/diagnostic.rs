use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub rule_id: String,
    pub message: String,
    pub file: PathBuf,
    pub line: usize,
    pub severity: Severity,
}
