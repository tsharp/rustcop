# rustcop-macros

Procedural macros for rustcop suppression directives.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
rustcop = "0.1.3"
```

The macro is re-exported from the main `rustcop` crate, so you don't need to add `rustcop-macros` separately.

## Usage

### Suppress all rules for an item

```rust
use rustcop::ignore;

#[rustcop::ignore]
fn my_function() {
    // This function will not be checked by rustcop
}
```

### Suppress specific rules

```rust
#[rustcop::ignore(RC1001, RC1002)]
fn my_other_function() {
    // Only RC1001 and RC1002 will be suppressed for this function
}
```

### Suppress at module level

```rust
#![rustcop::ignore]

// Entire module is excluded from rustcop checks
fn foo() {}
fn bar() {}
```

### Alternative: Comment-based suppressions

You can also use comment-based suppressions if you prefer:

```rust
// rustcop:ignore-file
// Suppresses all rules for the entire file

// rustcop:ignore
fn foo() {
    // Suppresses all rules for the next line
}

// rustcop:ignore RC1001, RC1002
fn bar() {
    // Suppresses specific rules for the next line
}
```

## Supported Items

The `#[rustcop::ignore]` attribute can be applied to:
- Functions
- Modules
- Structs
- Enums
- Traits
- Implementations
- Constants
- Statics

## How it Works

The macro itself is transparent - it doesn't modify your code. Instead, rustcop's suppression parser reads the source code and detects the `#[rustcop::ignore]` attributes or comment directives, then filters out matching diagnostics during the check phase.

This means you get:
- Full IDE support with syntax highlighting
- Type-safe attribute syntax
- No runtime overhead
- Works with all rustcop rules

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../LICENSE-MIT))

at your option.
