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

Import formatting follows `rustfmt` conventions:
- **Grouping** — standard library first, then third-party crates, then internal (`crate`/`self`/`super`), separated by blank lines.
- **Sorting** — alphabetical within each group.
- **Merging** — multiple imports from the same crate are combined into a single `use` block.
- **Multi-line expansion** — imports with nested braces are expanded to one item per line.

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
