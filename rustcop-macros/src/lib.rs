//! Procedural macros for rustcop suppression directives.
//!
//! This crate provides the `#[rustcop::ignore]` attribute macro that allows
//! suppression of specific rustcop warnings and errors.
//!
//! # Examples
//!
//! Suppress all rustcop diagnostics for a function:
//! ```ignore
//! #[rustcop::ignore]
//! fn my_function() {
//!     // This will not be checked by rustcop
//! }
//! ```
//!
//! Suppress a specific rule with justification:
//! ```ignore
//! #[rustcop::ignore(RC1001, justification = "Legacy code, will refactor in v2")]
//! fn my_function() {
//!     // Only RC1001 will be suppressed
//! }
//! ```
//!
//! Stack multiple suppressions with different justifications:
//! ```ignore
//! #[rustcop::ignore(RC1001, justification = "Performance optimization")]
//! #[rustcop::ignore(RC1002, justification = "Required for API compatibility")]
//! fn my_function() {
//!     // RC1001 and RC1002 are both suppressed with separate justifications
//! }
//! ```
//!
//! Suppress at module level:
//! ```ignore
//! #![rustcop::ignore]
//! // Entire module is excluded from rustcop checks
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Expr, ExprLit, Item, Lit, Meta, MetaNameValue, Token,
};

/// Arguments for the rustcop::ignore attribute
#[allow(dead_code)]
struct IgnoreArgs {
    rule_code: Option<String>,
    justification: Option<String>,
}

impl Parse for IgnoreArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut rule_code = None;
        let mut justification = None;

        // Parse comma-separated arguments
        let args = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;

        for meta in args {
            match meta {
                // Simple path like RC1001
                Meta::Path(path) => {
                    if let Some(ident) = path.get_ident() {
                        rule_code = Some(ident.to_string());
                    }
                }
                // Named value like justification = "reason"
                Meta::NameValue(MetaNameValue { path, value, .. }) => {
                    if path.is_ident("justification") {
                        if let Expr::Lit(ExprLit {
                            lit: Lit::Str(s), ..
                        }) = value
                        {
                            justification = Some(s.value());
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(IgnoreArgs {
            rule_code,
            justification,
        })
    }
}

/// Attribute macro to suppress rustcop diagnostics.
///
/// Can be applied to functions, modules, structs, enums, traits, and impls.
///
/// # Usage
///
/// Without arguments (suppresses all rules):
/// ```ignore
/// #[rustcop::ignore]
/// fn foo() {}
/// ```
///
/// With a specific rule code:
/// ```ignore
/// #[rustcop::ignore(RC1001)]
/// fn bar() {}
/// ```
///
/// With justification:
/// ```ignore
/// #[rustcop::ignore(RC1001, justification = "Legacy API compatibility")]
/// fn baz() {}
/// ```
///
/// Multiple suppressions with different justifications:
/// ```ignore
/// #[rustcop::ignore(RC1001, justification = "Reason 1")]
/// #[rustcop::ignore(RC1002, justification = "Reason 2")]
/// fn qux() {}
/// ```
///
/// At module level:
/// ```ignore
/// #![rustcop::ignore]
/// ```
#[proc_macro_attribute]
pub fn ignore(args: TokenStream, input: TokenStream) -> TokenStream {
    // Parse the arguments
    let _args = parse_macro_input!(args as IgnoreArgs);

    // Parse the item being annotated (could be function, module, struct, etc.)
    // We don't actually modify anything - the suppression system parses this from source
    let item = parse_macro_input!(input as Item);

    // Return the item unchanged
    // The actual suppression is handled by parsing the attribute in suppression.rs
    let output = quote! {
        #item
    };

    TokenStream::from(output)
}

/// Inner attribute macro for module-level suppression.
///
/// This is syntactic sugar that works the same as `#[rustcop::ignore]`
/// but uses the inner attribute syntax `#![rustcop::ignore]`.
///
/// # Example
///
/// ```ignore
/// #![rustcop::ignore]
///
/// // Entire module is excluded from rustcop checks
/// fn foo() {}
/// fn bar() {}
/// ```
#[proc_macro_attribute]
pub fn ignore_module(_args: TokenStream, input: TokenStream) -> TokenStream {
    // For inner attributes, just pass through the input
    // The suppression parser handles detection from source
    input
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_ignore_compiles() {
        // These tests verify the macro compiles correctly
        // Actual suppression behavior is tested in the main rustcop crate
    }
}
