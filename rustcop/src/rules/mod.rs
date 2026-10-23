pub mod comment_spacing;
pub mod exports;
pub mod imports;
pub mod item_comments;
pub mod method_comments;
pub mod modules;
pub mod public_fields;
pub mod super_imports;
pub mod trait_impl_comments;
pub mod wildcard_imports;
use std::cmp::Ordering;
use std::path::Path;

use crate::diagnostic::{Diagnostic, Severity};

pub(super) fn configured_severity(value: &str) -> Option<Severity> {
    match value {
        "none" => None,
        "error" => Some(Severity::Error),
        _ => Some(Severity::Warning),
    }
}

pub(crate) fn configured_lint_severity_or(
    config: &crate::config::Config,
    name: &str,
    default: Severity,
) -> Option<Severity> {
    let configured = config.raw().get("lints").and_then(|lints| lints.get(name));
    if configured.is_none() {
        return Some(default);
    }

    let lint = config
        .get_nested_config::<crate::config::LintConfig>(&["lints", name])
        .unwrap_or_default();
    configured_severity(&lint.severity)
}

pub(super) fn format_visibility(vis: &syn::Visibility) -> String {
    match vis {
        syn::Visibility::Public(_) => "pub".to_string(),
        syn::Visibility::Restricted(restricted) => {
            let path = restricted
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            if restricted.in_token.is_some() {
                format!("pub(in {path})")
            } else {
                format!("pub({path})")
            }
        }
        syn::Visibility::Inherited => String::new(),
    }
}

pub(super) fn compare_visibilities(left: &str, right: &str) -> Ordering {
    visibility_sort_key(left).cmp(&visibility_sort_key(right))
}

fn visibility_sort_key(visibility: &str) -> (u8, &str) {
    let rank = match visibility {
        "" => 0,
        "pub(self)" => 1,
        "pub(super)" => 2,
        "pub(crate)" => 3,
        visibility if visibility.starts_with("pub(in ") => 4,
        "pub" => 5,
        _ => 4,
    };

    (rank, visibility)
}

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
mod documentation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_severity_uses_the_rule_default_when_unconfigured() {
        let severity = configured_lint_severity_or(
            &crate::config::Config::empty(),
            "example",
            Severity::Error,
        );
        assert!(matches!(severity, Some(Severity::Error)));
    }

    #[test]
    fn lint_severity_can_be_overridden_or_disabled() {
        let mut config = crate::config::Config::empty();
        config.raw = toml::from_str(
            r#"
            [lints.warning_rule]
            severity = "warning"

            [lints.disabled_rule]
            severity = "none"
            "#,
        )
        .unwrap();

        assert!(matches!(
            configured_lint_severity_or(&config, "warning_rule", Severity::Error),
            Some(Severity::Warning)
        ));
        assert!(configured_lint_severity_or(&config, "disabled_rule", Severity::Error).is_none());
    }
}
