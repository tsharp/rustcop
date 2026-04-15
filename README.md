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
[imports]
enabled = true
group = true    # Group imports: std → external → crate/self/super
sort = true     # Sort imports alphabetically within each group
merge = true    # Merge imports from the same crate into one `use` block
```

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
