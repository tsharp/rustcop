pub mod imports;
pub mod modules;
pub mod super_imports;
pub mod wildcard_imports;
use std::path::Path;

use crate::diagnostic::Diagnostic;

/// A lint/style rule that can check source files and optionally auto-fix violations.
pub trait Rule {
    /// Unique rule identifier (e.g., "RC1001").
    fn id(&self) -> &str;

    /// Human-readable rule name.
    fn name(&self) -> &str;

    /// Check file content for violations and return diagnostics.
    fn check(&self, content: &str, file: &Path) -> Vec<Diagnostic>;

    /// Return the fixed content. If no fix is needed, returns the original content unchanged.
    fn fix(&self, content: &str) -> String;
}
