# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

## Project Overview

`oagrep` - A Rust CLI tool for filtering OpenAPI specifications (currently in
early development).

## Build Commands

```powershell
# Build the project
cargo build

# Build for release
cargo build --release

# Run the application
cargo run

# Run tests
cargo test

# Run a single test
cargo test test_name

# Check code without building
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy
```

## Project Structure

- `src/main.rs` - Application entry point
- `Cargo.toml` - Project manifest and dependencies

## Other Notes

When updating markdown files, ensure proper formatting and adherence to the
prettier config.
