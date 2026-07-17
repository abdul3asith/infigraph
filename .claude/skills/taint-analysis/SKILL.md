---
name: taint-analysis
description: How Infigraph's taint analysis works — line-based intra-procedural tracking, the sanitizer proximity heuristic, inter-procedural BFS mode, and how to add a new source/sink/sanitizer. Use when working on crates/infigraph-core/src/taint/, adding a taint source/sink pattern, or investigating taint false positives/negatives.
---

# Taint analysis

Core engine: `crates/infigraph-core/src/taint/mod.rs` (intra-procedural), `interprocedural.rs` (cross-function BFS). Pattern tables: `sources.rs`, `sinks.rs`, sanitizer patterns alongside sinks. `concerns/` and `reflection/` are separate, simpler pattern-table scanners (authz/validation/caching annotations; dynamic dispatch detection) — not part of the taint engine itself, don't confuse the two when extending either.

## Important: this is line-based, not AST-based or truly dataflow-sensitive

The intra-procedural analyzer scans function bodies **line by line**, not via AST traversal. It tracks a `HashMap<var, TaintInfo>` of currently-tainted variables and propagates taint through a heuristic assignment parser (`extract_lhs`/`parse_assignment`) that handles `let`/`var`/`const` declarations, type annotations, and distinguishes `=` from `==` by string pattern, not real parsing. Taint clears on a variable when a sanitizer pattern appears on the assignment's RHS.

**Sink-sanitization is a proximity heuristic, not real dataflow**: `is_sanitized_nearby` checks whether a sanitizer pattern appears within **±5 lines** of a sink to decide if the sink is "covered." This is a real gotcha — a sanitizer 6 lines away, or one that sanitizes a *different* variable that happens to be near the sink, can produce a false negative or false positive respectively. If you're adding a sanitizer expecting precise flow-sensitivity (sanitizer applies to variable X, only sink using X is suppressed), that's **not what this does** — calibrate expectations before "fixing" what looks like an inaccurate heuristic; it may be working as designed, just coarse-grained by construction.

## Adding a new source/sink/sanitizer

Sources/sinks/sanitizers are plain per-language string pattern tables keyed by category (`SqlInjection`, `XssRisk`, etc.) in `sources.rs`/`sinks.rs`. **The contract**: a sink and its sanitizer must share the same `category` string — `TAINT_SINKS` and `TAINT_SANITIZERS` are matched by this shared key, not by any structural relationship. Adding a sink without a matching-category sanitizer entry means that sink can never be marked sanitized, even if an obviously-relevant sanitizer exists elsewhere under a different category string.

## Intra-procedural vs inter-procedural

- **Intra-procedural** (`mod.rs`) — single-function line scan as described above. Produces flows with full path/sanitization detail (source line, sink line, sanitized bool).
- **Inter-procedural** (`interprocedural.rs`) — BFS over `CALLS` edges (depth-bounded by `max_depth`), connecting a function containing a taint source to a function containing a sink, across function boundaries. Has a cached-adjacency variant for performance on large graphs. **Coarser granularity**: inter-procedural flows carry no path/sanitization info — they only report that *some* connection exists within the depth bound, not whether it's actually sanitized along the way. Don't expect the same precision (or lack thereof) between the two modes; they answer different questions ("is this line's value tainted at this sink" vs "could taint from function A's source reach function B's sink within N calls").

## Known failure modes (by design, not necessarily bugs)

- String-substring pattern matching for sources/sinks/sanitizers — no real AST or type information, so a sink-looking string inside a comment or string literal can false-positive, and a renamed/aliased sink function can false-negative.
- The ±5-line sanitizer proximity heuristic (above) is the single most common source of both false positives and false negatives reported against this system.
- No true dataflow graph — if you're tempted to "fix" a false positive/negative by making the heuristic smarter, consider whether the fix actually needs real AST-based dataflow tracking instead, which is a much bigger change than adjusting the line-proximity window.
