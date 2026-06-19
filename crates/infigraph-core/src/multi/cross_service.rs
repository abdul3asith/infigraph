use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::graph::GraphQuery;
use crate::lang::LanguageRegistry;
use crate::Infigraph;

use super::{ContractKind, Registry};

/// A cross-service dependency: service A calls service B at a specific route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossServiceDep {
    pub caller_service: String,
    pub caller_file: String,
    pub caller_symbol: String,
    pub target_service: String,
    pub target_method: String,
    pub target_path: String,
    pub url_found: String,
}

/// Detect cross-service HTTP dependencies within a group.
/// Scans source files for URL strings (fetch, http.get, requests.get, etc.)
/// and matches them to known contracts/routes in other services.
pub fn detect_cross_service_deps(
    registry: &Registry,
    group_name: &str,
    build_registry: impl Fn() -> Result<LanguageRegistry>,
) -> Result<Vec<CrossServiceDep>> {
    let group = registry
        .groups
        .get(group_name)
        .context(format!("group '{}' not found", group_name))?;

    // Collect all contracts as lookup table: path → (service, method)
    let mut route_lookup: HashMap<String, (String, String)> = HashMap::new();
    for contract in &group.contracts {
        if contract.kind == ContractKind::HttpRoute {
            // Normalize path for matching (strip params)
            let normalized = normalize_route_path(&contract.path);
            route_lookup.insert(
                normalized,
                (contract.service.clone(), contract.method.clone()),
            );
        }
    }

    let mut deps = Vec::new();

    for repo_name in &group.repos {
        let entry = match registry.repos.get(repo_name) {
            Some(e) => e.clone(),
            None => continue,
        };

        let lang_registry = build_registry()?;
        let mut prism = Infigraph::open(&entry.path, lang_registry)?;
        prism.init()?;

        let store = match prism.store() {
            Some(s) => s,
            None => continue,
        };
        let conn = match store.connection() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let gq = GraphQuery::new(&conn);

        // Find symbols with URL-like strings in docstrings or search source files
        let rows = gq.raw_query(
            "MATCH (s:Symbol) WHERE s.docstring IS NOT NULL AND (s.docstring CONTAINS '/api/' OR s.docstring CONTAINS 'http://' OR s.docstring CONTAINS 'https://') RETURN s.id, s.name, s.file, s.docstring",
        ).unwrap_or_default();

        for row in &rows {
            let doc = row.get(3).map(|s| s.as_str()).unwrap_or("");
            let urls = extract_api_paths(doc);
            for url in urls {
                let normalized = normalize_route_path(&url);
                if let Some((target_svc, target_method)) = route_lookup.get(&normalized) {
                    if target_svc != repo_name {
                        deps.push(CrossServiceDep {
                            caller_service: repo_name.clone(),
                            caller_file: row[2].clone(),
                            caller_symbol: row[0].clone(),
                            target_service: target_svc.clone(),
                            target_method: target_method.clone(),
                            target_path: url.clone(),
                            url_found: url,
                        });
                    }
                }
            }
        }

        // Also grep source files for URL patterns
        let source_urls = scan_source_for_urls(&entry.path);
        for (file, symbol_hint, url) in source_urls {
            let normalized = normalize_route_path(&url);
            if let Some((target_svc, target_method)) = route_lookup.get(&normalized) {
                if target_svc != repo_name {
                    // Try to resolve line hint to enclosing symbol ID
                    let caller_id = if let Some(stripped) = symbol_hint.strip_prefix("line:") {
                        let line_num: i32 = stripped.parse().unwrap_or(0);
                        let escaped_file = file.replace('\'', "\\'");
                        let q = format!(
                            "MATCH (s:Symbol) WHERE s.file = '{}' AND s.start_line <= {} AND s.end_line >= {} RETURN s.id ORDER BY (s.end_line - s.start_line) ASC LIMIT 1",
                            escaped_file, line_num, line_num
                        );
                        gq.raw_query(&q)
                            .ok()
                            .and_then(|rows| rows.into_iter().next())
                            .and_then(|row| row.into_iter().next())
                            .unwrap_or_else(|| format!("{}:{}", file, symbol_hint))
                    } else {
                        symbol_hint.clone()
                    };
                    deps.push(CrossServiceDep {
                        caller_service: repo_name.clone(),
                        caller_file: file,
                        caller_symbol: caller_id,
                        target_service: target_svc.clone(),
                        target_method: target_method.clone(),
                        target_path: url.clone(),
                        url_found: url,
                    });
                }
            }
        }
    }

    Ok(deps)
}

/// Link cross-service HTTP dependencies as CALLS_SERVICE edges in each caller's graph.
/// Returns number of edges created.
pub fn link_cross_service_calls(
    registry: &Registry,
    group_name: &str,
    build_registry: impl Fn() -> Result<LanguageRegistry>,
) -> Result<usize> {
    let deps = detect_cross_service_deps(registry, group_name, &build_registry)?;
    if deps.is_empty() {
        return Ok(0);
    }

    // Group deps by caller service
    let mut by_caller: HashMap<String, Vec<&CrossServiceDep>> = HashMap::new();
    for dep in &deps {
        by_caller
            .entry(dep.caller_service.clone())
            .or_default()
            .push(dep);
    }

    let mut total = 0;

    for (caller_svc, svc_deps) in &by_caller {
        let entry = match registry.repos.get(caller_svc) {
            Some(e) => e,
            None => continue,
        };

        let lang_registry = build_registry()?;
        let mut prism = Infigraph::open(&entry.path, lang_registry)?;
        prism.init()?;

        let store = match prism.store() {
            Some(s) => s,
            None => continue,
        };
        let _lock = match store.write_lock() {
            Ok(l) => l,
            Err(_) => continue,
        };
        let conn = match store.connection() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let gq = GraphQuery::new(&conn);

        for dep in svc_deps {
            let target_id = format!(
                "xsvc::{}::{}::{}",
                dep.target_service,
                dep.target_method,
                dep.target_path.replace('\'', "\\'")
            );
            let target_name = format!(
                "{} {} {}",
                dep.target_service, dep.target_method, dep.target_path
            )
            .replace('\'', "\\'");
            let caller_sym = dep.caller_symbol.replace('\'', "\\'");
            let target_svc = dep.target_service.replace('\'', "\\'");
            let target_method = dep.target_method.replace('\'', "\\'");
            let target_path = dep.target_path.replace('\'', "\\'");

            // Create ExternalService node — only use columns from Symbol schema.
            // Use MERGE for idempotency (safe to run group_link multiple times).
            let docstring = format!(
                "External service: {} {} {}",
                target_svc, target_method, target_path
            );
            let create_target = format!(
                "MERGE (t:Symbol {{id: '{}'}}) \
                 ON CREATE SET t.name = '{}', t.kind = 'ExternalService', \
                 t.file = '(external)', t.start_line = 0, t.end_line = 0, \
                 t.signature_hash = '', t.language = 'external', t.visibility = 'public', \
                 t.parent = '', t.docstring = '{}', t.complexity = 0",
                target_id, target_name, docstring,
            );
            let _ = gq.raw_query(&create_target);

            // Check if edge already exists before creating (idempotent)
            let check_edge = format!(
                "MATCH (caller:Symbol {{id: '{}'}})-[:CALLS_SERVICE]->(target:Symbol {{id: '{}'}}) RETURN caller.id",
                caller_sym, target_id,
            );
            let existing = gq.raw_query(&check_edge).unwrap_or_default();
            if !existing.is_empty() {
                continue;
            }

            let create_edge = format!(
                "MATCH (caller:Symbol {{id: '{}'}}), (target:Symbol {{id: '{}'}}) \
                 CREATE (caller)-[:CALLS_SERVICE {{method: '{}', path: '{}', target_service: '{}'}}]->(target)",
                caller_sym, target_id, target_method, target_path, target_svc,
            );
            if gq.raw_query(&create_edge).is_ok() {
                total += 1;
            }
        }
    }

    Ok(total)
}

/// Normalize a route path for matching: strip trailing slash, remove param placeholders.
fn normalize_route_path(path: &str) -> String {
    let path = path.trim_end_matches('/');
    // Extract just the path portion from full URLs
    let path = if let Some(idx) = path.find("/api/") {
        &path[idx..]
    } else if path.starts_with("http") {
        path.split("//")
            .nth(1)
            .and_then(|s| s.find('/').map(|i| &s[i..]))
            .unwrap_or(path)
    } else {
        path
    };
    // Normalize path params: /users/:id → /users/{id} → /users/*
    let segments: Vec<&str> = path.split('/').collect();
    segments
        .iter()
        .map(|s| {
            if s.starts_with(':') || s.starts_with('{') || s.starts_with('<') {
                "*"
            } else {
                s
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Extract API paths from a string (URL literals in code).
fn extract_api_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for part in text
        .split('"')
        .chain(text.split('\'').chain(text.split('`')))
    {
        let trimmed = part.trim();
        if (trimmed.starts_with("/api/") || trimmed.starts_with("http"))
            && trimmed.contains("/api/")
        {
            paths.push(trimmed.to_string());
        }
    }
    paths
}

/// Scan source files for URL strings containing /api/ patterns.
fn scan_source_for_urls(root: &Path) -> Vec<(String, String, String)> {
    const SKIP_DIRS: &[&str] = &[
        ".infigraph",
        ".git",
        "node_modules",
        "target",
        "build",
        "dist",
        "__pycache__",
        ".venv",
    ];
    let mut results = Vec::new();
    walk_for_urls(root, root, SKIP_DIRS, &mut results);
    results
}

fn walk_for_urls(
    base: &Path,
    dir: &Path,
    skip: &[&str],
    results: &mut Vec<(String, String, String)>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if !skip.contains(&name_str.as_ref()) && !name_str.starts_with('.') {
                walk_for_urls(base, &path, skip, results);
            }
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for (line_num, line) in content.lines().enumerate() {
                for delim in ['"', '\'', '`'] {
                    for part in line.split(delim) {
                        let trimmed = part.trim();
                        if trimmed.contains("/api/")
                            && trimmed.len() < 200
                            && !trimmed.contains(' ')
                        {
                            let path_part = if trimmed.starts_with("http") {
                                trimmed
                                    .split("//")
                                    .nth(1)
                                    .and_then(|s| s.find('/').map(|i| &s[i..]))
                                    .unwrap_or(trimmed)
                            } else {
                                trimmed
                            };
                            if path_part.starts_with("/api/") {
                                results.push((
                                    rel.clone(),
                                    format!("line:{}", line_num + 1),
                                    path_part.to_string(),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}
