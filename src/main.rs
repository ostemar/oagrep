use clap::Parser;
use serde_json::{Map, Value};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "oagrep")]
#[command(about = "Filter OpenAPI JSON specifications to reduce context size")]
struct Cli {
    /// Path pattern filter (e.g., /api/v1/users/*)
    #[arg(short = 'p')]
    pattern: Option<String>,

    /// Output to file (writes to stdout if not provided)
    #[arg(short = 'o')]
    output: Option<PathBuf>,

    /// Keep all schemas (disable automatic pruning of unreferenced schemas)
    #[arg(long = "keep-schemas")]
    keep_schemas: bool,

    /// Strip description fields from entire spec
    #[arg(long = "strip-descriptions")]
    strip_descriptions: bool,

    /// Strip example/examples fields from entire spec
    #[arg(long = "strip-examples")]
    strip_examples: bool,

    /// Minimal info section (keep only title and version)
    #[arg(long = "minimal-info")]
    minimal_info: bool,

    /// Apply all strip options for maximum size reduction
    #[arg(short = 'm', long = "minimal")]
    minimal: bool,

    /// OpenAPI JSON file or URL (reads from stdin if not provided)
    input: Option<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn fetch_url(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let response = client.get(url).send()?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()).into());
    }
    Ok(response.text()?)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Read input
    let input = match &cli.input {
        Some(s) if is_url(s) => fetch_url(s)?,
        Some(path) => fs::read_to_string(path)?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    // Parse JSON
    let mut spec: Value = serde_json::from_str(&input)?;

    // Filter paths if pattern provided
    if let Some(pattern) = &cli.pattern {
        filter_openapi(&mut spec, pattern);
    }

    // Prune unused schemas (on by default, disable with --keep-schemas)
    if !cli.keep_schemas {
        let paths = spec.get("paths").cloned().unwrap_or(Value::Null);
        let direct_refs: HashSet<String> = extract_refs(&paths)
            .into_iter()
            .filter_map(|r| parse_schema_ref(&r).map(String::from))
            .collect();

        let schemas = spec
            .get("components")
            .and_then(|c| c.get("schemas"))
            .and_then(|s| s.as_object())
            .cloned()
            .unwrap_or_default();

        let keep = resolve_schema_deps(&schemas, direct_refs);
        prune_schemas(&mut spec, &keep);
    }

    // Apply strip options
    if cli.strip_descriptions || cli.minimal {
        strip_descriptions(&mut spec);
    }
    if cli.strip_examples || cli.minimal {
        strip_examples(&mut spec);
    }
    if cli.minimal_info || cli.minimal {
        minimal_info(&mut spec);
    }

    // Write output
    let output = serde_json::to_string_pretty(&spec)?;
    match &cli.output {
        Some(path) => fs::write(path, output)?,
        None => io::stdout().write_all(output.as_bytes())?,
    }

    Ok(())
}

fn filter_openapi(spec: &mut Value, pattern: &str) {
    if let Some(obj) = spec.as_object_mut() {
        if let Some(paths) = obj.get_mut("paths") {
            if let Some(paths_obj) = paths.as_object_mut() {
                let filtered: Map<String, Value> = paths_obj
                    .iter()
                    .filter(|(path, _)| matches_pattern(path, pattern))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                *paths = Value::Object(filtered);
            }
        }
    }
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    let path_segments: Vec<&str> = path.split('/').collect();
    let pattern_segments: Vec<&str> = pattern.split('/').collect();

    // Check if pattern ends with ** (matches zero or more segments)
    let ends_with_globstar = pattern_segments.last() == Some(&"**");

    // If pattern has more segments than path, it can only match if ** can consume zero segments
    if pattern_segments.len() > path_segments.len() {
        if ends_with_globstar && pattern_segments.len() == path_segments.len() + 1 {
            // ** can match zero segments - check all preceding segments
            for (i, pat_seg) in pattern_segments
                .iter()
                .take(pattern_segments.len() - 1)
                .enumerate()
            {
                if *pat_seg == "*" {
                    continue;
                } else if *pat_seg != path_segments[i] {
                    return false;
                }
            }
            return true;
        }
        return false;
    }

    for (i, pat_seg) in pattern_segments.iter().enumerate() {
        if *pat_seg == "*" {
            // Single * matches any single segment
            continue;
        } else if *pat_seg == "**" {
            // ** matches zero or more segments - match everything remaining
            return true;
        } else if *pat_seg != path_segments[i] {
            return false;
        }
    }

    // Pattern consumed - must have matched all segments exactly
    // (single * doesn't extend matching beyond pattern length)
    pattern_segments.len() == path_segments.len()
}

/// Recursively extract all $ref values from a JSON Value
fn extract_refs(value: &Value) -> HashSet<String> {
    let mut refs = HashSet::new();
    extract_refs_recursive(value, &mut refs);
    refs
}

fn extract_refs_recursive(value: &Value, refs: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(ref_str)) = map.get("$ref") {
                refs.insert(ref_str.clone());
            }
            for v in map.values() {
                extract_refs_recursive(v, refs);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                extract_refs_recursive(v, refs);
            }
        }
        _ => {}
    }
}

/// Parse "#/components/schemas/Activity" -> Some("Activity")
fn parse_schema_ref(ref_str: &str) -> Option<&str> {
    ref_str.strip_prefix("#/components/schemas/")
}

/// Find all schemas transitively referenced from a starting set
fn resolve_schema_deps(
    schemas: &Map<String, Value>,
    direct_refs: HashSet<String>,
) -> HashSet<String> {
    let mut resolved = HashSet::new();
    let mut queue: VecDeque<String> = direct_refs.into_iter().collect();

    while let Some(schema_name) = queue.pop_front() {
        if resolved.contains(&schema_name) {
            continue;
        }
        resolved.insert(schema_name.clone());

        if let Some(schema_value) = schemas.get(&schema_name) {
            let nested_refs = extract_refs(schema_value);
            for ref_str in nested_refs {
                if let Some(nested_name) = parse_schema_ref(&ref_str) {
                    if !resolved.contains(nested_name) {
                        queue.push_back(nested_name.to_string());
                    }
                }
            }
        }
    }

    resolved
}

/// Remove all schemas not in the keep set
fn prune_schemas(spec: &mut Value, keep: &HashSet<String>) {
    if let Some(components) = spec.get_mut("components").and_then(|c| c.as_object_mut()) {
        if let Some(schemas) = components
            .get_mut("schemas")
            .and_then(|s| s.as_object_mut())
        {
            schemas.retain(|name, _| keep.contains(name));
        }
    }
}

/// Recursively remove a key from all objects in the JSON tree
fn strip_key_recursive(value: &mut Value, key: &str) {
    match value {
        Value::Object(map) => {
            map.remove(key);
            for v in map.values_mut() {
                strip_key_recursive(v, key);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                strip_key_recursive(v, key);
            }
        }
        _ => {}
    }
}

fn strip_descriptions(spec: &mut Value) {
    strip_key_recursive(spec, "description");
}

fn strip_examples(spec: &mut Value) {
    strip_key_recursive(spec, "example");
    strip_key_recursive(spec, "examples");
}

fn minimal_info(spec: &mut Value) {
    if let Some(info) = spec.get_mut("info").and_then(|i| i.as_object_mut()) {
        let title = info.get("title").cloned();
        let version = info.get("version").cloned();
        info.clear();
        if let Some(t) = title {
            info.insert("title".to_string(), t);
        }
        if let Some(v) = version {
            info.insert("version".to_string(), v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(matches_pattern("/api/users", "/api/users"));
        assert!(!matches_pattern("/api/users", "/api/orders"));
    }

    #[test]
    fn test_single_wildcard() {
        assert!(matches_pattern("/api/users", "/api/*"));
        assert!(matches_pattern("/api/orders", "/api/*"));
        assert!(!matches_pattern("/api/users/123", "/api/*"));
    }

    #[test]
    fn test_wildcard_in_middle() {
        assert!(matches_pattern("/api/v1/users", "/api/*/users"));
        assert!(matches_pattern("/api/v2/users", "/api/*/users"));
        assert!(!matches_pattern("/api/v1/orders", "/api/*/users"));
    }

    #[test]
    fn test_double_wildcard() {
        assert!(matches_pattern("/api/users", "/api/**"));
        assert!(matches_pattern("/api/users/123", "/api/**"));
        assert!(matches_pattern("/api/users/123/profile", "/api/**"));
    }

    #[test]
    fn test_filter_openapi() {
        let mut spec: Value = serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/api/users": {"get": {}},
                "/api/users/{id}": {"get": {}},
                "/api/orders": {"get": {}},
                "/api/orders/{id}": {"get": {}}
            }
        });

        filter_openapi(&mut spec, "/api/users/**");

        let paths = spec["paths"].as_object().unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains_key("/api/users"));
        assert!(paths.contains_key("/api/users/{id}"));
        assert!(!paths.contains_key("/api/orders"));
    }

    #[test]
    fn test_extract_refs() {
        let json = serde_json::json!({
            "responses": {
                "200": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "$ref": "#/components/schemas/User"
                            }
                        }
                    }
                }
            },
            "requestBody": {
                "content": {
                    "application/json": {
                        "schema": {
                            "$ref": "#/components/schemas/CreateUser"
                        }
                    }
                }
            }
        });
        let refs = extract_refs(&json);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains("#/components/schemas/User"));
        assert!(refs.contains("#/components/schemas/CreateUser"));
    }

    #[test]
    fn test_parse_schema_ref() {
        assert_eq!(parse_schema_ref("#/components/schemas/User"), Some("User"));
        assert_eq!(
            parse_schema_ref("#/components/schemas/Activity"),
            Some("Activity")
        );
        assert_eq!(parse_schema_ref("#/components/parameters/Id"), None);
        assert_eq!(parse_schema_ref("invalid"), None);
    }

    #[test]
    fn test_resolve_schema_deps_transitive() {
        let schemas: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "User": {"properties": {"address": {"$ref": "#/components/schemas/Address"}}},
            "Address": {"properties": {"country": {"$ref": "#/components/schemas/Country"}}},
            "Country": {"type": "string"},
            "Unused": {"type": "object"}
        }))
        .unwrap();

        let direct: HashSet<String> = ["User".to_string()].into_iter().collect();
        let resolved = resolve_schema_deps(&schemas, direct);

        assert!(resolved.contains("User"));
        assert!(resolved.contains("Address"));
        assert!(resolved.contains("Country"));
        assert!(!resolved.contains("Unused"));
    }

    #[test]
    fn test_resolve_schema_deps_circular() {
        let schemas: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "Node": {"properties": {"child": {"$ref": "#/components/schemas/Node"}}},
            "Other": {"type": "string"}
        }))
        .unwrap();

        let direct: HashSet<String> = ["Node".to_string()].into_iter().collect();
        let resolved = resolve_schema_deps(&schemas, direct);

        assert!(resolved.contains("Node"));
        assert!(!resolved.contains("Other"));
    }

    #[test]
    fn test_prune_schemas() {
        let mut spec = serde_json::json!({
            "components": {
                "schemas": {
                    "User": {"type": "object"},
                    "Order": {"type": "object"},
                    "Unused": {"type": "object"}
                }
            }
        });

        let keep: HashSet<String> = ["User".to_string()].into_iter().collect();
        prune_schemas(&mut spec, &keep);

        let schemas = spec["components"]["schemas"].as_object().unwrap();
        assert_eq!(schemas.len(), 1);
        assert!(schemas.contains_key("User"));
        assert!(!schemas.contains_key("Order"));
        assert!(!schemas.contains_key("Unused"));
    }

    #[test]
    fn test_strip_descriptions() {
        let mut spec = serde_json::json!({
            "info": {"title": "API", "description": "An API"},
            "paths": {"/foo": {"description": "Foo endpoint", "get": {"description": "Get foo"}}}
        });
        strip_descriptions(&mut spec);
        assert!(spec["info"].get("description").is_none());
        assert!(spec["paths"]["/foo"].get("description").is_none());
        assert!(spec["paths"]["/foo"]["get"].get("description").is_none());
    }

    #[test]
    fn test_strip_examples() {
        let mut spec = serde_json::json!({
            "components": {
                "schemas": {
                    "User": {
                        "type": "object",
                        "example": {"id": 1},
                        "properties": {
                            "name": {"type": "string", "example": "John"}
                        }
                    }
                }
            }
        });
        strip_examples(&mut spec);
        assert!(spec["components"]["schemas"]["User"]
            .get("example")
            .is_none());
        assert!(spec["components"]["schemas"]["User"]["properties"]["name"]
            .get("example")
            .is_none());
    }

    #[test]
    fn test_minimal_info() {
        let mut spec = serde_json::json!({
            "info": {
                "title": "My API",
                "version": "1.0.0",
                "description": "A long description",
                "contact": {"email": "test@test.com"},
                "license": {"name": "MIT"}
            }
        });
        minimal_info(&mut spec);
        let info = spec["info"].as_object().unwrap();
        assert_eq!(info.len(), 2);
        assert_eq!(info.get("title").unwrap(), "My API");
        assert_eq!(info.get("version").unwrap(), "1.0.0");
        assert!(info.get("description").is_none());
        assert!(info.get("contact").is_none());
    }

    #[test]
    fn test_full_pipeline_with_pruning() {
        let mut spec = serde_json::json!({
            "openapi": "3.0.0",
            "paths": {
                "/users": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {"$ref": "#/components/schemas/User"}
                                    }
                                }
                            }
                        }
                    }
                },
                "/orders": {
                    "get": {
                        "responses": {
                            "200": {
                                "content": {
                                    "application/json": {
                                        "schema": {"$ref": "#/components/schemas/Order"}
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "User": {"type": "object", "properties": {"address": {"$ref": "#/components/schemas/Address"}}},
                    "Address": {"type": "object"},
                    "Order": {"type": "object"},
                    "Unused": {"type": "object"}
                }
            }
        });

        // Filter to /users only
        filter_openapi(&mut spec, "/users/**");

        // Prune schemas
        let paths = spec.get("paths").cloned().unwrap_or(Value::Null);
        let direct_refs: HashSet<String> = extract_refs(&paths)
            .into_iter()
            .filter_map(|r| parse_schema_ref(&r).map(String::from))
            .collect();

        let schemas = spec["components"]["schemas"].as_object().unwrap().clone();
        let keep = resolve_schema_deps(&schemas, direct_refs);
        prune_schemas(&mut spec, &keep);

        // Verify results
        let schemas = spec["components"]["schemas"].as_object().unwrap();
        assert_eq!(schemas.len(), 2); // User and Address (transitive)
        assert!(schemas.contains_key("User"));
        assert!(schemas.contains_key("Address"));
        assert!(!schemas.contains_key("Order"));
        assert!(!schemas.contains_key("Unused"));
    }

    #[test]
    fn test_is_url_http() {
        assert!(is_url("http://example.com/api.json"));
    }

    #[test]
    fn test_is_url_https() {
        assert!(is_url("https://example.com/api.json"));
    }

    #[test]
    fn test_is_url_file_path() {
        assert!(!is_url("./api.json"));
        assert!(!is_url("/path/to/api.json"));
        assert!(!is_url("C:\\path\\to\\api.json"));
    }

    #[test]
    #[ignore] // Run with: cargo test -- --ignored
    fn test_fetch_real_url() {
        let result = fetch_url("https://tripletex.no/v2/openapi.json");
        assert!(result.is_ok());
        let content = result.unwrap();
        assert!(content.contains("openapi"));
    }
}
