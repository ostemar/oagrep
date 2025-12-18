# oagrep

A CLI tool for filtering OpenAPI JSON specifications to reduce context size when working with coding agents.

## Rationale

Large OpenAPI specifications can consume too much context when used with AI coding agents for tasks like generating API tests or clients. This tool extracts only the endpoints you need.

## Usage

```
oagrep [OPTIONS] [FILE]

Arguments:
  [FILE]  OpenAPI JSON file (reads from stdin if not provided)

Options:
  -p <PATTERN>  Path pattern filter (e.g., /api/v1/users/*)
  -o <FILE>     Output to file (writes to stdout if not provided)
```

## Examples

```bash
# Filter to specific path pattern
oagrep -p "/api/v1/users/*" api.json

# Pipe through with filter
cat api.json | oagrep -p "/orders/*"

# Filter and write to file
oagrep -p "/api/v1/products/*" api.json -o filtered.json
```

## Technical Details

- Uses `serde` / `serde_json` for efficient JSON parsing
- Supports stdin/stdout for easy pipeline integration

## Roadmap

- URL input with streaming download via `reqwest` for memory-efficient processing of remote specs
