# RustCop

An experimental Rust style linter and formatter inspired by C#'s StyleCop.

RustCop checks and auto-fixes style violations in your Rust source files, starting with import statement formatting.

## Installation

```sh
cargo install rustcop
```

## Usage

### Check for violations

```sh
cargo rustcop check [paths...]
```

Scans `.rs` files and reports style violations without modifying anything. Exits with code 1 if any diagnostics are found.

### Auto-fix violations

```sh
cargo rustcop fix [paths...]
```

Fixes violations in-place and runs a verification pass to confirm everything is clean.

If no paths are given, both commands default to the current directory.

### Options

| Flag | Description |
|------|-------------|
| `-c`, `--config <FILE>` | Path to the config file (default: `rustcop.toml`) |

## Configuration

Create a `rustcop.toml` in your project root:

```toml
version = 1

# When true, warnings are treated as errors (default: false)
treat_warnings_as_errors = false

# When true, suppression directives must include justifications (default: true)
require_suppression_justification = true

[imports]
enabled = true
group = true    # Group imports: std → external → crate/self/super
sort = true     # Sort imports alphabetically within each group
merge = true    # Merge imports from the same crate into one `use` block
```

## Suppression Directives

RustCop supports both comment-based and attribute-based suppression directives, similar to C#'s `[SuppressMessage]`.

### Comment-Based Suppressions

```rust
// Suppress all rules for the entire file
// rustcop:ignore-file

// Suppress all rules for the next line
// rustcop:ignore
fn my_function() { }

// Suppress a specific rule with justification (recommended)
// rustcop:ignore RC1001: Legacy code, will refactor in v2.0
fn another_function() { }

// Suppress multiple rules with a shared justification
// rustcop:ignore RC1001, RC1002: Performance critical section
fn performance_function() { }

// Stack multiple suppressions for different justifications per rule
// rustcop:ignore RC1001: Reason for RC1001
// rustcop:ignore RC1002: Reason for RC1002
fn complex_function() { }
```

### Attribute-Based Suppressions

```rust
// Suppress all rules for a function
#[rustcop::ignore]
fn my_function() { }

// Suppress a specific rule with justification
#[rustcop::ignore(RC1001, justification = "Legacy API compatibility")]
fn another_function() { }

// Stack multiple attributes for different justifications per rule
#[rustcop::ignore(RC1001, justification = "Performance optimization")]
#[rustcop::ignore(RC1002, justification = "Required for backwards compatibility")]
fn complex_function() { }

// Suppress at module level
#![rustcop::ignore]
```

The attribute macro is re-exported from the main `rustcop` crate:

```rust
use rustcop::ignore;

#[rustcop::ignore]
fn my_function() { }
```

See [examples/suppression_demo.rs](examples/suppression_demo.rs) for more examples.

## Structured Output

RustCop can output diagnostics in machine-readable formats:

```sh
# Generate SARIF output
cargo rustcop check --out results.sarif

# Generate JSON output
cargo rustcop check --out results.json
```

The format is automatically detected from the file extension. SARIF v2.1.0 is supported.

## Rules

| ID | Name | Description |
|----|------|-------------|
| RC1001 | ImportFormatting | Groups, sorts, and merges `use` statements |
| RC2001 | DisallowSuperImports | Reports use of `super::` imports when configured |
| RC2003 | DisallowPublicFields | Reports unrestricted `pub` struct fields when configured |
| RC3001 | RequireMethodComments | Requires documentation on all impl and trait methods when configured |
| RC3002 | RequireItemComments | Requires documentation on selected declarations and individual enum variants |
| RC3003 | RequireTraitImplComments | Requires documentation on trait impl blocks; defaults to RC3001's configured severity |
| RC9001 | UnusedSuppression | Reports suppression directives that don't suppress any diagnostics |
| RC9002 | MissingSuppressionJustification | Reports suppressions without justifications when `require_suppression_justification = true` |

Import formatting follows `rustfmt` conventions:
- **Grouping** — standard library first, then third-party crates, then internal (`crate`/`self`/`super`), separated by blank lines.
- **Sorting** — alphabetical within each group.
- **Merging** — multiple imports from the same crate are combined into a single `use` block.
- **Multi-line expansion** — imports with nested braces are expanded to one item per line.

**Unused Suppression Detection:**
RustCop generates an error (RC9001) for any suppression directive that doesn't actually suppress any diagnostics. This helps keep your codebase clean by detecting:
- Suppressions added for issues that have since been fixed
- Suppressions targeting the wrong line or rule
- Suppressions for rules that don't exist or aren't enabled

**Suppression Justification Enforcement:**
When `require_suppression_justification = true` in your config, RustCop generates an error (RC9002) for any suppression that lacks a justification comment. This helps maintain documentation of why rules are being suppressed.

**Super Import Validation:**
When `[lints.disallow_super_imports]` is configured with `severity = "error"`, RustCop generates an error (RC2001) for any use of `super::` imports. This can be useful for enforcing explicit import paths.

## Public struct fields

To limit field visibility on named and tuple structs, enable:

```toml
[lints.disallow_public_fields]
severity = "error"
allowed_visibility = "crate"
```

The rule is disabled by default. `warning` reports warnings; `none` disables it.
`allowed_visibility` controls which explicitly visible fields are permitted:

| Value | Allowed field visibility |
|-------|--------------------------|
| `crate` (default) | Private fields and any restricted visibility, including `pub(crate)` |
| `module` | Private fields, `pub(self)`, `pub(super)`, and `pub(in path)` restricted to a module; excludes `pub(crate)` and `pub(in crate)` |
| `none` | Private fields only, without a `pub` modifier |

Plain `pub` is disallowed in every mode. The module mode checks the written
restriction; it does not resolve whether a parent module is the crate root.
It checks structs in inline modules and function bodies, including private
structs, but does not expand macros. It does not automatically remove `pub`,
because that can break callers.

Suppress a single field with a comment immediately before its `pub` line:

```rust
struct Settings {
    // rustcop::ignore RC2003: Required by the existing API
    pub name: String,
    value: usize,
}
```

Suppress every public field on one struct:

```rust
#[rustcop::ignore(RC2003, justification = "Public data transfer object")]
struct Message {
    pub id: u64,
    pub text: String,
}
```

A `// rustcop::ignore RC2003: reason` comment immediately before the struct
header also suppresses its public fields. Use comments for field exceptions:
Rust does not support procedural attribute macros directly on fields.
Exceptions retain the existing unused-suppression and justification checks.

## Method documentation

Require documentation on inherent methods and trait method declarations,
including private methods, associated functions such as `new`, and default
trait method definitions:

```toml
[lints.require_method_comments]
severity = "error"
require_trait_method_docs = false
```

RC3001 is disabled by default. It accepts nonempty `///`, `/** ... */`, and
`#[doc = "..."]` documentation, as well as `#[doc = include_str!(...)]`.
Ordinary comments, empty documentation, and `#[doc(hidden)]` do not satisfy it.
Methods in `impl Trait for Type` automatically use the trait documentation
policy and do not require their own comments by default. Methods generated by macros
are outside this rule's scope. Documentation is never auto-generated by `fix`.

```rust
impl Message {
    /// Return the message identifier.
    fn id(&self) -> u64 { self.id }

    // rustcop::ignore RC3001: Legacy compatibility method
    fn legacy(&self) {}
}
```

Per-method exceptions also accept
`#[rustcop::ignore(RC3001, justification = "reason")]` immediately before the
method signature. Exceptions retain unused-suppression and justification checks.

### Trait implementations and inherited documentation

RustCop detects trait implementations directly from `impl Trait for Type`;
no inheritance marker is needed. It recognizes the inheritance relationship
without resolving the trait or checking whether an external trait actually has
documentation. Local trait method declarations are still checked by RC3001,
and local trait declarations by RC3002 when its `traits` category is enabled.

The defaults when RC3001 is enabled are:

| Declaration | Own documentation required? |
|-------------|-----------------------------|
| Inherent method or associated function | Yes |
| Trait method declaration, including default bodies | Yes |
| Method in a trait implementation | No |
| Trait implementation block | Yes, by RC3003 |

```rust
/// Display a message using its identifier.
impl Display for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id)
    }
}
```

Set `require_trait_method_docs = true` under `[lints.require_method_comments]`
to require implementation-specific documentation on each explicitly written
trait method as well. Methods using a trait's default body without an override
have no local declaration to check. Inheritance markers do not satisfy this
stricter requirement or RC3003.

Configure impl documentation separately, including when RC3001 is disabled:

```toml
[lints.require_trait_impl_comments]
severity = "error" # "none" disables the impl-block requirement
```

If that section is omitted, RC3003 uses RC3001's severity; all documentation
rules remain disabled when none is configured. Suppress one impl with
`// rustcop::ignore RC3003: reason` immediately before its `impl` line, or the
existing `#[rustcop::ignore(RC3003, justification = "reason")]` attribute.
An impl-block exception does not suppress missing method documentation.

The earlier `inherit_trait_docs` option remains accepted for compatibility:
`true` makes trait impl method documentation optional and `false` requires it.
The new `require_trait_method_docs` setting takes precedence when both exist.

## Declaration documentation

RC3002 requires documentation on module-level declarations, including private
ones. Enable all supported categories by omitting `items`, or select a subset:

```toml
[lints.require_item_comments]
severity = "error"
items = [
    "structs", "enums", "enum_variants", "traits", "functions",
    "modules", "constants", "statics", "type_aliases",
]
```

The rule is disabled by default. `items = []` disables all categories.
It checks inline modules recursively, but excludes declarations inside function
bodies, impl/trait associated items, imports, and generated macro output.
Method documentation remains controlled by RC3001. Modules may use inner
`//!` documentation or outer `///` documentation.

Enums and their variants require independent documentation:

```rust
/// The result of processing a message.
enum Outcome {
    /// Processing completed successfully.
    Success,
    /// Processing failed with a reason.
    Failure(String),
}
```

Comment exceptions (`// rustcop::ignore RC3002: reason`) apply individually
to the declaration or variant immediately following them. A suppression on an
enum does not suppress its variants. On declarations that support procedural
attributes, `#[rustcop::ignore(RC3002, justification = "reason")]` also works;
use comment exceptions on enum variants. Inheritance markers alone cannot
satisfy RC3002 because these declarations have no implicit parent contract.

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
