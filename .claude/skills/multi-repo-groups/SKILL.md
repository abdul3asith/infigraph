---
name: multi-repo-groups
description: How Infigraph's multi-repo/group mode works — HTTP contract extraction heuristics, cross-service CALLS_SERVICE edge linking, the combined-graph merge pipeline, and remote (Neo4j/Postgres) vs local mode differences. Use when working on crates/infigraph-core/src/multi/, adding a web framework's contract detection, or debugging group build/query/deps.
---

# Multi-repo groups

Core logic lives in `crates/infigraph-core/src/multi/`. This is one of the least AST-driven subsystems — much of the contract-extraction and cross-service-linking logic is string/regex heuristics rather than tree-sitter queries.

## Contract extraction

Tiered, most-confident-first: use an already-detected `Route` symbol if one exists, else fall back to decorator/docstring pattern matching, else scrape router-prefix patterns from raw source. Only add lower-tier patterns for frameworks the route-detection pass doesn't already cover.

## Cross-service linking

Matches dynamic URLs (including templated/interpolated ones) against known routes, with self-match suppression and method-preference fallback, to build `CALLS_SERVICE` edges across repos.

## Combined-graph merge

Per-repo graphs are merged via a bulk export/prefix/import pipeline rather than row-by-row inserts, for performance at scale. ID-prefixing conventions differ between node and edge tables — a new node/edge type needs to follow the existing convention exactly, or it'll produce dangling references silently.

## Remote mode

`--features remote` swaps the graph store and registry backends and enables parallel multi-repo indexing (impossible in local single-writer mode). Any new `multi/` feature should consider whether it needs a remote-mode branch, or it may silently no-op or violate single-writer assumptions under concurrent writes.
