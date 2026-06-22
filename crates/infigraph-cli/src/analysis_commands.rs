use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use infigraph_core::Infigraph;
use infigraph_languages::bundled_registry;

pub(crate) fn cmd_architecture(root: &Path) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let conn = store.connection()?;
    let gq = infigraph_core::graph::GraphQuery::new(&conn);

    let report = build_architecture_report(&gq)?;
    println!("{}", report);
    Ok(())
}

fn build_architecture_report(gq: &infigraph_core::graph::GraphQuery) -> Result<String> {
    let mut out = String::new();

    // 1. Language breakdown
    out.push_str("=== Language Breakdown ===\n");
    let lang_rows =
        gq.raw_query("MATCH (m:Module) RETURN m.language, count(m) ORDER BY count(m) DESC")?;
    if lang_rows.is_empty() {
        out.push_str("  (no modules indexed)\n");
    } else {
        for row in &lang_rows {
            out.push_str(&format!("  {:>20}: {} files\n", row[0], row[1]));
        }
    }

    // 2. Total symbols by kind
    out.push_str("\n=== Symbols by Kind ===\n");
    let kind_rows =
        gq.raw_query("MATCH (s:Symbol) RETURN s.kind, count(s) ORDER BY count(s) DESC")?;
    if kind_rows.is_empty() {
        out.push_str("  (no symbols indexed)\n");
    } else {
        for row in &kind_rows {
            out.push_str(&format!("  {:>20}: {}\n", row[0], row[1]));
        }
    }

    // 3. Hotspots: files with most symbols
    out.push_str("\n=== Hotspot Files (most symbols) ===\n");
    let hotspot_rows =
        gq.raw_query("MATCH (s:Symbol) RETURN s.file, count(s) AS cnt ORDER BY cnt DESC LIMIT 10")?;
    if hotspot_rows.is_empty() {
        out.push_str("  (no symbols indexed)\n");
    } else {
        for (i, row) in hotspot_rows.iter().enumerate() {
            out.push_str(&format!(
                "  {:>2}. {:60} {} symbols\n",
                i + 1,
                row[0],
                row[1]
            ));
        }
    }

    // 4. Hub functions: most-called
    out.push_str("\n=== Hub Functions (most callers) ===\n");
    let hub_rows = gq.raw_query(
        "MATCH ()-[r:CALLS]->(s:Symbol) RETURN s.name, s.file, count(r) AS calls ORDER BY calls DESC LIMIT 10",
    )?;
    if hub_rows.is_empty() {
        out.push_str("  (no call edges found)\n");
    } else {
        for (i, row) in hub_rows.iter().enumerate() {
            out.push_str(&format!(
                "  {:>2}. {:30} {:40} {} callers\n",
                i + 1,
                row[0],
                row[1],
                row[2]
            ));
        }
    }

    // 5. Entry points: functions that call others but are not called themselves
    out.push_str("\n=== Entry Points (call others, never called) ===\n");
    let entry_rows = gq.raw_query(
        "MATCH (s:Symbol)-[:CALLS]->() WHERE s.kind IN ['Function', 'Method'] AND NOT EXISTS { MATCH ()-[:CALLS]->(s) } RETURN DISTINCT s.name, s.kind, s.file ORDER BY s.file, s.name LIMIT 20",
    )?;
    if entry_rows.is_empty() {
        out.push_str("  (none found)\n");
    } else {
        for row in &entry_rows {
            out.push_str(&format!("  {:>8} {:30} {}\n", row[1], row[0], row[2]));
        }
    }

    Ok(out)
}

pub(crate) fn cmd_cluster(root: &Path) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let _lock = store.write_lock()?;
    let conn = store.connection()?;

    println!("Running Louvain community detection...");
    let stats = infigraph_core::cluster::detect_clusters(&conn)?;
    println!("{}", stats);
    Ok(())
}

pub(crate) fn cmd_detect_changes(root: &Path, base: &str, depth: u32) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let conn = store.connection()?;
    let gq = infigraph_core::graph::GraphQuery::new(&conn);

    let report = build_detect_changes_report(prism.root(), &gq, base, depth)?;
    println!("{}", report);
    Ok(())
}

/// Parse git diff output and map changed lines to symbols in the graph.
fn build_detect_changes_report(
    project_root: &std::path::Path,
    gq: &infigraph_core::graph::GraphQuery,
    base: &str,
    depth: u32,
) -> Result<String> {
    // 1. Get changed files
    let name_output = std::process::Command::new("git")
        .args(["diff", "--name-only", base])
        .current_dir(project_root)
        .output()
        .context("failed to run git diff --name-only")?;

    if !name_output.status.success() {
        let stderr = String::from_utf8_lossy(&name_output.stderr);
        anyhow::bail!("git diff failed: {}", stderr.trim());
    }

    let changed_files: Vec<String> = String::from_utf8_lossy(&name_output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    if changed_files.is_empty() {
        return Ok("No changes detected.".to_string());
    }

    // 2. Get unified diff with zero context to extract changed line ranges
    let diff_output = std::process::Command::new("git")
        .args(["diff", "--unified=0", base])
        .current_dir(project_root)
        .output()
        .context("failed to run git diff --unified=0")?;

    let diff_text = String::from_utf8_lossy(&diff_output.stdout);
    let hunks = parse_diff_hunks(&diff_text);

    // 3. For each changed file+range, find overlapping symbols
    let mut directly_changed: Vec<(String, String, String, u32, u32)> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for (file, start, end) in &hunks {
        let symbols = gq.symbols_in_range(file, *start, *end)?;
        for s in symbols {
            if seen_ids.insert(s.id.clone()) {
                directly_changed.push((s.id, s.name, s.file, s.start_line, s.end_line));
            }
        }
    }

    let mut out = String::new();
    out.push_str(&format!("=== Change Detection (base: {}) ===\n\n", base));
    out.push_str(&format!("Changed files: {}\n", changed_files.len()));
    for f in &changed_files {
        out.push_str(&format!("  {}\n", f));
    }

    out.push_str(&format!(
        "\n=== Directly Changed Symbols ({}) ===\n",
        directly_changed.len()
    ));
    if directly_changed.is_empty() {
        out.push_str("  (no indexed symbols overlap with changed lines)\n");
    } else {
        for (id, name, file, start, end) in &directly_changed {
            out.push_str(&format!("  {:30} {} L{}-{}\n", name, file, start, end));
            let _ = id;
        }
    }

    // 4. Compute blast radius via transitive impact for each directly changed symbol
    if !directly_changed.is_empty() && depth > 0 {
        let mut indirectly_affected: Vec<(String, String, String, String)> = Vec::new();
        let mut indirect_ids: HashSet<String> = HashSet::new();

        for (id, _, _, _, _) in &directly_changed {
            if let Ok(impacted) = gq.transitive_impact(id, depth) {
                for row in impacted {
                    if !seen_ids.contains(&row.id) && indirect_ids.insert(row.id.clone()) {
                        indirectly_affected.push((row.id, row.name, row.file, row.kind));
                    }
                }
            }
        }

        out.push_str(&format!(
            "\n=== Blast Radius (depth={}, {} indirectly affected) ===\n",
            depth,
            indirectly_affected.len()
        ));
        if indirectly_affected.is_empty() {
            out.push_str("  (no additional symbols affected)\n");
        } else {
            for (_, name, file, kind) in &indirectly_affected {
                out.push_str(&format!("  {:>8} {:30} {}\n", kind, name, file));
            }
        }
    }

    Ok(out)
}

/// Parse unified diff output (with --unified=0) to extract (file, start_line, end_line) hunks.
fn parse_diff_hunks(diff: &str) -> Vec<(String, u32, u32)> {
    let mut hunks = Vec::new();
    let mut current_file = String::new();

    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = path.to_string();
            continue;
        }

        if line.starts_with("@@") && !current_file.is_empty() {
            if let Some(plus_part) = line.split('+').nth(1) {
                let range_part = plus_part.split(' ').next().unwrap_or("");
                let parts: Vec<&str> = range_part.split(',').collect();
                let start: u32 = parts[0].parse().unwrap_or(0);
                let count: u32 = if parts.len() > 1 {
                    parts[1].parse().unwrap_or(1)
                } else {
                    1
                };
                if start > 0 {
                    let end = if count == 0 { start } else { start + count - 1 };
                    hunks.push((current_file.clone(), start, end));
                }
            }
        }
    }

    hunks
}

pub(crate) fn cmd_security(
    root: &Path,
    severity: Option<&str>,
    category: Option<&str>,
) -> Result<()> {
    let canonical = root.canonicalize().context("invalid project root")?;
    let mut scan = infigraph_core::security::scan_project(&canonical)?;

    if let Some(sev) = severity {
        let sev_upper = sev.to_uppercase();
        scan.findings
            .retain(|f| f.severity.to_string() == sev_upper);
    }
    if let Some(cat) = category {
        let cat_norm = cat.to_lowercase().replace(' ', "");
        scan.findings
            .retain(|f| f.category.to_string().to_lowercase().replace(' ', "") == cat_norm);
    }

    println!("{}", infigraph_core::security::format_scan_results(&scan));
    Ok(())
}

pub(crate) fn cmd_complexity(root: &Path, threshold: u32, file: Option<&str>) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let conn = store.connection()?;
    let gq = infigraph_core::graph::GraphQuery::new(&conn);

    let base_q = if let Some(f) = file {
        format!(
            "MATCH (s:Symbol) WHERE (s.kind = 'Function' OR s.kind = 'Method' OR s.kind = 'Test') AND s.file CONTAINS '{}' RETURN s.name, s.file, s.start_line, s.complexity ORDER BY s.complexity DESC",
            f.replace('\'', "\\'")
        )
    } else {
        "MATCH (s:Symbol) WHERE (s.kind = 'Function' OR s.kind = 'Method' OR s.kind = 'Test') RETURN s.name, s.file, s.start_line, s.complexity ORDER BY s.complexity DESC".to_string()
    };

    let rows = gq.raw_query(&base_q)?;
    if rows.is_empty() {
        println!("No symbols found. Run 'infigraph index' first.");
        return Ok(());
    }

    let total: u32 = rows
        .iter()
        .filter_map(|r| r.get(3).and_then(|v| v.parse::<u32>().ok()))
        .sum();
    let avg = total as f64 / rows.len() as f64;
    let hotspots: Vec<_> = rows
        .iter()
        .filter(|r| r.get(3).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) >= threshold)
        .collect();

    println!(
        "Complexity: {} symbols, avg {:.1}, {} hotspots (>= {})\n",
        rows.len(),
        avg,
        hotspots.len(),
        threshold
    );

    for row in rows.iter().take(30) {
        let name = row.first().map(|s| s.as_str()).unwrap_or("?");
        let file = row.get(1).map(|s| s.as_str()).unwrap_or("?");
        let line = row.get(2).map(|s| s.as_str()).unwrap_or("?");
        let cplx = row.get(3).map(|s| s.as_str()).unwrap_or("0");
        let flag = if cplx.parse::<u32>().unwrap_or(0) >= threshold {
            " ⚠"
        } else {
            ""
        };
        println!("  [{cplx:>3}] {name}  ({file}:{line}){flag}");
    }
    Ok(())
}

pub(crate) fn cmd_refactor(
    root: &Path,
    target: Option<&str>,
    focus: &str,
    limit: usize,
) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("not initialized")?;
    let conn = store.connection()?;

    let emb_path = root.join(".infigraph").join("embeddings.bin");
    let emb_ref = if emb_path.exists() {
        Some(emb_path.as_path())
    } else {
        None
    };

    let focus = infigraph_core::refactor::Focus::parse(focus);
    let recs = infigraph_core::refactor::analyze(&conn, emb_ref, target, focus, limit)?;
    print!(
        "{}",
        infigraph_core::refactor::format_recommendations(&recs, target)
    );
    Ok(())
}

pub(crate) fn cmd_semantic_diff(root: &Path, old_ref: &str, new_ref: &str) -> Result<()> {
    let canonical = root.canonicalize().context("invalid project root")?;
    let registry = bundled_registry()?;
    let diff = infigraph_core::diff::semantic_diff(&canonical, old_ref, new_ref, &registry)?;
    println!("{}", infigraph_core::diff::format_diff(&diff));
    Ok(())
}

pub(crate) fn cmd_sequence(root: &Path, symbol_id: &str, depth: u32) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;
    let store = prism.store().context("not initialized")?;
    let conn = store.connection()?;
    let gq = infigraph_core::graph::GraphQuery::new(&conn);
    let diagram = infigraph_core::sequence::generate_sequence_mermaid(&gq, symbol_id, depth)?;
    println!("{}", diagram);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_review(
    root: &Path,
    base: &str,
    limit: usize,
    json: bool,
    llm: bool,
    dry_run: bool,
    context: Option<&str>,
    group: Option<&str>,
) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;
    let store = prism
        .store()
        .context("graph not initialized -- run 'infigraph index' first")?;

    let report = if let Some(group_name) = group {
        let multi_reg = infigraph_core::multi::Registry::load()?;
        infigraph_core::review::review_with_group(
            root,
            base,
            limit,
            prism.registry(),
            store,
            group_name,
            &multi_reg,
            bundled_registry,
        )?
    } else {
        infigraph_core::review::review(root, base, limit, prism.registry(), store)?
    };

    if json && !llm {
        println!("{}", infigraph_core::review::format_review_json(&report));
    } else if !llm {
        print!("{}", infigraph_core::review::format_review(&report));
    }

    if llm || dry_run {
        use infigraph_core::review::llm;
        let (prompt, result) = llm::review_with_llm(root, &report, store, dry_run, context)?;

        if dry_run {
            println!("{}", prompt);
        } else if let Some(result) = result {
            if json {
                println!("{}", llm::format_llm_review_json(&result));
            } else {
                print!("{}", infigraph_core::review::format_review(&report));
                print!("{}", llm::format_llm_review(&result));
            }
        }
    }

    Ok(())
}

pub(crate) fn cmd_check(
    root: &Path,
    config: Option<&Path>,
    json: bool,
    checks: Option<&str>,
) -> Result<bool> {
    use infigraph_core::check::{self, CheckSelection, CheckStatus};

    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;
    let store = prism
        .store()
        .context("graph not initialized -- run 'infigraph index' first")?;

    let config_path = config
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| root.join(".infigraph").join("check.toml"));
    let cfg = check::load_config(&config_path)?;

    let selection = match checks {
        Some(csv) => CheckSelection::from_csv(csv),
        None => CheckSelection::all(),
    };

    let results = check::run_checks(root, &cfg, store, &selection);

    if json {
        println!("{}", check::format_json(&results));
    } else {
        print!("{}", check::format_table(&results));
    }

    let any_failed = results.iter().any(|r| r.status == CheckStatus::Fail);
    Ok(any_failed)
}

pub(crate) fn cmd_vulns(
    root: &Path,
    severity: Option<&str>,
    ecosystem: Option<&str>,
    json: bool,
) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;
    let store = prism.store().context("graph not initialized")?;

    let deps = infigraph_core::manifest::query_deps(store)?;
    if deps.is_empty() {
        println!("No dependencies found. Run 'infigraph index-manifests' first.");
        return Ok(());
    }

    eprintln!(
        "Scanning {} dependencies against OSV database...",
        deps.len()
    );

    let mut report = infigraph_core::vuln::scan_deps(&deps)?;

    if let Some(sev) = severity {
        infigraph_core::vuln::filter_by_severity(&mut report, sev);
    }
    if let Some(eco) = ecosystem {
        infigraph_core::vuln::filter_by_ecosystem(&mut report, eco);
    }

    if json {
        println!("{}", infigraph_core::vuln::format_json(&report));
    } else {
        print!("{}", infigraph_core::vuln::format_table(&report));
    }

    Ok(())
}

pub(crate) fn cmd_detect_patterns(root: &Path, pattern: Option<&str>, json: bool) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;
    let store = prism
        .store()
        .context("graph not initialized -- run 'infigraph index' first")?;

    let report = infigraph_core::patterns::detect_filtered(store, pattern)?;

    if json {
        println!("{}", infigraph_core::patterns::format_json(&report));
    } else {
        print!("{}", infigraph_core::patterns::format_report(&report));
    }

    Ok(())
}

pub(crate) fn cmd_forget(root: &Path) -> Result<()> {
    let abs_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut store = infigraph_core::learned::LearnedStore::load(&abs_root);
    let count = store.len();
    store.clear();
    store.save(&abs_root)?;
    println!("Cleared {} learned patterns", count);
    Ok(())
}

pub(crate) fn cmd_bridges(root: &Path, kind: Option<&str>) -> Result<()> {
    let canonical = root.canonicalize().context("invalid project root")?;
    let result = infigraph_core::bridges::detect_bridges(&canonical)?;

    let bridges: Vec<_> = match kind {
        Some(k) => {
            let k_upper = k.to_uppercase();
            result
                .bridges
                .iter()
                .filter(|b| b.kind.as_str() == k_upper)
                .collect()
        }
        None => result.bridges.iter().collect(),
    };

    if bridges.is_empty() {
        let filter_note = kind.map(|k| format!(" (filter: {k})")).unwrap_or_default();
        println!("No cross-language bridges detected{}.", filter_note);
        return Ok(());
    }

    println!("Cross-language bridges: {} total", result.bridges.len());

    // Group by file
    let mut by_file: std::collections::HashMap<&str, Vec<_>> = std::collections::HashMap::new();
    for b in &bridges {
        by_file.entry(&b.file).or_default().push(b);
    }
    let mut files: Vec<&str> = by_file.keys().copied().collect();
    files.sort_unstable();

    for file in files {
        let file_bridges = &by_file[file];
        println!("\n  {}:", file);
        let mut sorted = file_bridges.to_vec();
        sorted.sort_by_key(|b| b.line);
        for b in sorted {
            let target = b.target_language.as_deref().unwrap_or("unknown");
            println!(
                "    L{} [{}] {} -> {} | {}",
                b.line,
                b.kind.as_str(),
                b.foreign_symbol,
                target,
                b.detail
            );
        }
    }

    Ok(())
}

pub(crate) fn cmd_clones(root: &Path, threshold: f64, limit: usize) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let conn = store.connection()?;
    let gq = infigraph_core::graph::GraphQuery::new(&conn);

    let threshold_f32 = threshold as f32;
    let kinds = ["Function", "Method"];
    let kind_filter = kinds
        .iter()
        .map(|k| format!("s.kind = '{}'", k))
        .collect::<Vec<_>>()
        .join(" OR ");
    let query = format!(
        "MATCH (s:Symbol) WHERE ({kind_filter}) RETURN s.id, s.name, s.kind, s.file, s.docstring"
    );
    let rows = gq.raw_query(&query)?;

    if rows.len() < 2 {
        println!("Not enough symbols to compare. Run 'infigraph index' first.");
        return Ok(());
    }

    let embedder = infigraph_core::embed::best_embedder();
    let emb_path = root.join(".infigraph").join("embeddings.bin");

    let cached: std::collections::HashMap<String, Vec<f32>> = if emb_path.exists() {
        infigraph_core::embed::load_embeddings_cached(&emb_path)?
            .into_iter()
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    let symbol_vecs: Vec<(String, String, String, Vec<f32>)> = rows
        .iter()
        .map(|row| {
            let id = row[0].clone();
            let text = if row.get(4).is_some_and(|s| !s.is_empty()) {
                format!("{} {}: {}", row[2], row[1], row[4])
            } else {
                format!("{} {}", row[2], row[1])
            };
            let emb = cached
                .get(&id)
                .cloned()
                .unwrap_or_else(|| embedder.embed(&text).unwrap_or_default());
            (id, row[1].clone(), row[3].clone(), emb)
        })
        .filter(|(_, _, _, emb)| !emb.is_empty())
        .collect();

    let n = symbol_vecs.len();
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();

    for i in 0..n {
        for j in (i + 1)..n {
            if symbol_vecs[i].2 == symbol_vecs[j].2 {
                continue;
            }
            let sim =
                infigraph_core::embed::cosine_similarity(&symbol_vecs[i].3, &symbol_vecs[j].3);
            if sim >= threshold_f32 {
                pairs.push((sim, i, j));
            }
        }
    }

    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    pairs.truncate(limit);

    if pairs.is_empty() {
        println!(
            "No clones found above threshold {:.2} across {} symbols.",
            threshold, n
        );
        return Ok(());
    }

    // Write SIMILAR_TO edges
    let write_conn = store.connection()?;
    for (score, i, j) in &pairs {
        let id_a = &symbol_vecs[*i].0;
        let id_b = &symbol_vecs[*j].0;
        let escape = |s: &str| s.replace('\'', "\\'");
        let _ = write_conn.query(&format!(
            "MATCH (a:Symbol), (b:Symbol) WHERE a.id = '{}' AND b.id = '{}' \
             MERGE (a)-[r:SIMILAR_TO]->(b) SET r.score = {}",
            escape(id_a),
            escape(id_b),
            score
        ));
    }

    println!(
        "Clone detection: {} pairs found (threshold={:.2}, symbols={})\n",
        pairs.len(),
        threshold,
        n
    );

    for (score, i, j) in &pairs {
        let (id_a, name_a, file_a, _) = &symbol_vecs[*i];
        let (id_b, name_b, file_b, _) = &symbol_vecs[*j];
        println!(
            "  {:.3}  {} ({}) <-> {} ({})\n         {} vs {}",
            score, name_a, id_a, name_b, id_b, file_a, file_b
        );
    }

    Ok(())
}

pub(crate) fn cmd_concerns(root: &Path, kind: Option<&str>) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let matches = infigraph_core::concerns::detect_cross_cutting(store)?;

    let filtered: Vec<_> = if let Some(k) = kind {
        let k_lower = k.to_lowercase();
        matches
            .iter()
            .filter(|m| m.kind.to_lowercase() == k_lower)
            .cloned()
            .collect()
    } else {
        matches
    };

    println!("{}", infigraph_core::concerns::format_concerns(&filtered));
    Ok(())
}

pub(crate) fn cmd_config_bindings(
    root: &Path,
    kind: Option<&str>,
    profile: Option<&str>,
) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let bindings = infigraph_core::config::detect_config_bindings(store)?;
    let canonical = root.canonicalize().context("invalid project root")?;
    let config_files = infigraph_core::config::detect_config_files(&canonical);

    let filtered: Vec<_> = bindings
        .iter()
        .filter(|b| {
            kind.as_ref()
                .is_none_or(|k| b.kind.to_lowercase() == k.to_lowercase())
                && profile
                    .as_ref()
                    .is_none_or(|p| b.profile.to_lowercase() == p.to_lowercase())
        })
        .cloned()
        .collect();

    println!(
        "{}",
        infigraph_core::config::format_config_bindings(&filtered, &config_files)
    );
    Ok(())
}

pub(crate) fn cmd_reflection(root: &Path, mechanism: Option<&str>) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let canonical = root.canonicalize().context("invalid project root")?;
    let sites = infigraph_core::reflection::detect_reflection_sites(store, &canonical)?;

    let filtered: Vec<_> = if let Some(m) = mechanism {
        let m_lower = m.to_lowercase();
        sites
            .iter()
            .filter(|s| s.mechanism.to_lowercase() == m_lower)
            .cloned()
            .collect()
    } else {
        sites
    };

    println!(
        "{}",
        infigraph_core::reflection::format_reflection_sites(&filtered)
    );
    Ok(())
}

pub(crate) fn cmd_taint(
    root: &Path,
    category: Option<&str>,
    show_sanitized: bool,
    inter: bool,
    depth: u32,
) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let canonical = root.canonicalize().context("invalid project root")?;

    if inter {
        let flows = infigraph_core::taint::interprocedural::detect_interprocedural_taint(
            store, &canonical, depth,
        )?;

        let filtered: Vec<_> = if let Some(c) = category {
            let c_lower = c.to_lowercase();
            flows
                .iter()
                .filter(|f| f.sink_category.to_lowercase() == c_lower)
                .cloned()
                .collect()
        } else {
            flows
        };

        println!(
            "{}",
            infigraph_core::taint::interprocedural::format_interprocedural_flows(&filtered)
        );
    } else {
        let flows = infigraph_core::taint::detect_taint_flows(store, &canonical)?;

        let filtered: Vec<_> = flows
            .iter()
            .filter(|f| {
                category
                    .as_ref()
                    .is_none_or(|c| f.sink_category.to_lowercase() == c.to_lowercase())
                    && (show_sanitized || !f.sanitized)
            })
            .cloned()
            .collect();

        println!("{}", infigraph_core::taint::format_taint_flows(&filtered));
    }

    Ok(())
}

pub(crate) fn cmd_dynamic_urls(root: &Path) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let canonical = root.canonicalize().context("invalid project root")?;
    let urls = infigraph_core::taint::dynamic_urls::detect_dynamic_urls(store, &canonical)?;

    println!(
        "{}",
        infigraph_core::taint::dynamic_urls::format_dynamic_urls(&urls)
    );
    Ok(())
}

pub(crate) fn cmd_path_traversal(root: &Path, depth: u32) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;

    let store = prism.store().context("graph not initialized")?;
    let canonical = root.canonicalize().context("invalid project root")?;
    let flows =
        infigraph_core::taint::path_traversal::detect_path_traversal(store, &canonical, depth)?;

    println!(
        "{}",
        infigraph_core::taint::path_traversal::format_path_traversal(&flows)
    );
    Ok(())
}

pub(crate) fn cmd_bridges_promote(root: &Path) -> Result<()> {
    let registry = bundled_registry()?;
    let mut prism = Infigraph::open(root, registry)?;
    prism.init()?;
    let store = prism
        .store()
        .context("graph not initialized -- run 'infigraph index' first")?;
    let conn = store.connection()?;
    let gq = infigraph_core::graph::GraphQuery::new(&conn);

    // Find BRIDGE_TO edges where both endpoints are resolved symbols, promote to CALLS
    let bridge_rows =
        gq.raw_query("MATCH (a:Symbol)-[r:BRIDGE_TO]->(b:Symbol) RETURN a.id, b.id")?;

    if bridge_rows.is_empty() {
        println!("No BRIDGE_TO edges found to promote.");
        return Ok(());
    }

    let count = bridge_rows.len();
    for row in &bridge_rows {
        let _ = gq.raw_query(&format!(
            "MATCH (a:Symbol {{id: '{}'}})-[r:BRIDGE_TO]->(b:Symbol {{id: '{}'}}) DELETE r CREATE (a)-[:CALLS]->(b)",
            row[0].replace('\'', "\\'"),
            row[1].replace('\'', "\\'"),
        ));
    }
    println!("Promoted {} BRIDGE_TO edges to CALLS edges.", count);
    Ok(())
}
