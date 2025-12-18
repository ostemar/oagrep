use clap::Parser;
use serde_json::{Map, Value};
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

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

    /// OpenAPI JSON file (reads from stdin if not provided)
    file: Option<PathBuf>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Read input
    let input = match &cli.file {
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
}
