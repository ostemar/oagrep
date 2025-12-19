# oagrep

A CLI tool for filtering OpenAPI JSON specifications to reduce context size when
working with coding agents.

## Rationale

Large OpenAPI specifications can consume too much context when used with AI
coding agents for tasks like generating API tests or clients. This tool extracts
only the endpoints you need.

## Usage

```
oagrep [OPTIONS] [INPUT]

Arguments:
  [INPUT]  OpenAPI JSON file or URL (reads from stdin if not provided)

Options:
  -p <PATTERN>          Path pattern filter (e.g., /api/v1/users/*)
  -o <FILE>             Output to file (writes to stdout if not provided)
  --keep-schemas        Keep all schemas (disable automatic pruning)
  --strip-descriptions  Remove description fields from entire spec
  --strip-examples      Remove example/examples fields
  --minimal-info        Keep only title and version in info section
  -m, --minimal         Apply all strip options for maximum reduction
```

## Examples

```bash
# Filter to specific path pattern
oagrep -p "/api/v1/users/*" api.json

# Filter remote spec by URL
oagrep -p "/activity/**" https://tripletex.no/v2/openapi.json

# Pipe through with filter
cat api.json | oagrep -p "/orders/*"

# Filter and write to file
oagrep -p "/api/v1/products/*" api.json -o filtered.json
```

## Efficiency

The tool automatically prunes unreferenced schemas after filtering endpoints.
This dramatically reduces output size.

**Example: [Tripletex API](https://tripletex.no/v2/openapi.json) filtered to
`/activity/**`\*\*

| Version                 | Size      | Reduction |
| ----------------------- | --------- | --------- |
| Original spec           | 3.5 MB    | -         |
| Path filtering only     | 2.5 MB    | 28%       |
| **With schema pruning** | **23 KB** | **99%**   |
| With `--minimal`        | 12 KB     | 99.7%     |

Schema pruning finds all `$ref` references in the filtered endpoints, resolves
transitive dependencies, and removes all unreferenced schemas.

## Technical Details

- Uses `serde` / `serde_json` for efficient JSON parsing
- Uses `reqwest` with blocking client for URL fetching
- Supports stdin/stdout for easy pipeline integration
- Automatic schema pruning with transitive dependency resolution
- Handles circular schema references

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file
for details.
