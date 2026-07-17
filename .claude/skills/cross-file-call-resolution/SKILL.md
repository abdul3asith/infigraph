---
name: cross-file-call-resolution
description: How Infigraph resolves a bare function call to its actual definition across files — the layered matching strategy (learned cache, receiver-aware, enclosing-class, import-scope), ambiguity handling, and how SCIP enrichment overrides/corrects tree-sitter-derived edges. Use when debugging unresolved calls, wrong cross-file resolution, or extending resolve/.
---

# Cross-file call resolution

Entry points: `crates/infigraph-core/src/resolve/calls.rs` — `resolve_calls()` (full index) and `resolve_calls_incremental()` (incremental; builds the symbol map from the *entire* graph via `store.get_all_symbols()`, not just changed files, so incremental runs don't lose cross-file links). Both delegate to `resolve_with_map()` then `resolve_inherits()` (in `inherits.rs`, same strategy, restricted to `TYPE_KINDS = ["Class","Interface","Struct","Trait","Enum"]`).

## Matching strategy, in exact order, per dangling call (per-file, parallel via `rayon`)

1. **Learned-store lookup first** — `learned_store.lookup(&ext.file, target_name)`. If a prior SCIP-derived correction exists with confidence ≥ 0.3 and its target still exists, use it immediately.
2. **Receiver-aware resolution** — if the call captured a `.receiver` (from `obj.method()` syntax), look up `"ClassName::method"` in a prebuilt `class_method_map`. Exactly one match → use it. Multiple matches → prefer one whose file stem matches an imported module (derived from `Imports` relations in the caller's file), else fall back to the lexicographically-shortest symbol id.
3. **Enclosing-class preference** (no receiver, or receiver lookup failed) — gather all `symbol_map[target_name]` candidates from *other files* (same-file matches excluded; SQL-function false positives excluded when the call site is inside SQL). Exactly one candidate → use it. Multiple candidates, in order:
   - prefer one whose id contains `::<receiver>::<name>` (receiver pattern), else
   - prefer one in the caller's own enclosing class (`::<caller_class>::` substring match), else
   - `import_scope_match()`: prefer a candidate whose file stem is among the caller's imported modules; if none and the call site is SQL, prefer `Class`-kind candidates; otherwise **the call is left unresolved**.

**No guessing on ambiguity**: if none of the tie-breakers produce a unique answer, the edge is not created — it's counted in `ResolveStats` as unresolved, not silently pointed at an arbitrary same-named candidate.

**Persistence**: resolved pairs are bulk-loaded via a temp Parquet file + `COPY CALLS FROM`, with a fallback to chunked `UNWIND ... CREATE (a)-[:CALLS]->(b)` Cypher if the COPY fails.

## SCIP enrichment overrides tree-sitter resolution

`crates/infigraph-core/src/scip/mod.rs::import_scip_index()`, pass 2 (building CALLS from SCIP reference occurrences): before adding a SCIP-derived CALLS edge, it checks the pre-loaded `existing_calls` map (tree-sitter-created edges) for the same `container_id`. If tree-sitter had pointed the same call name at a *different* target, this is logged as a correction — `learned_store.record_correction(source_file, call_name, target_file, &target_id)` — and `stats.corrections_learned` increments.

**SCIP always augments, never deletes**: the new SCIP-derived edge is added regardless of the discrepancy. The old tree-sitter edge is left in place; the correction is recorded for future runs, not applied retroactively to existing graph state.

## Learned-resolution cache

`crates/infigraph-core/src/learned/mod.rs` — persisted at `.infigraph/learned/patterns.json`, separate from the graph DB (so it survives `infigraph index --full`, which wipes `.infigraph/` but preserves this like it preserves sessions).

- `LearnedPattern { source_file, call_name, resolved_to_file, resolved_to_symbol, confidence, source, last_updated }`.
- New correction starts at confidence `0.5`; repeated identical corrections bump it `+0.1`, capped at `1.0`.
- `lookup()` only returns patterns at confidence `>= 0.3`.
- `prune_stale()` drops patterns whose target file no longer exists in the index.

**Practical effect**: once SCIP has corrected a call once, a later *plain tree-sitter* reindex (no SCIP re-run) reproduces the SCIP-quality resolution from the learned cache — layer 1 in the matching order above always checks it first.

## Debugging an unresolved or wrong cross-file call

- Check `ResolveStats` output from `infigraph index` (printed as `{resolve_stats}`) for unresolved-call counts.
- If a call resolves to the *wrong* file: check for multiple same-named candidates and whether the caller's imports/enclosing class actually disambiguate it — if not, this is expected-unresolved-or-ambiguous behavior, not a bug, unless you can add a real tie-breaker signal.
- If SCIP "fixes" a resolution but a later reindex reverts it: check `.infigraph/learned/patterns.json` wasn't wiped and that confidence hasn't decayed below 0.3 (it doesn't currently decay over time — only via explicit re-correction — so if it reverted, check the pattern was actually written, not a decay assumption).
