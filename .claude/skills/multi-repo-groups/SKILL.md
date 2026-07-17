---
name: multi-repo-groups
description: How Infigraph's multi-repo/group mode works — HTTP contract extraction heuristics, cross-service CALLS_SERVICE edge linking, the Parquet-based combined-graph merge pipeline, and remote (Neo4j/Postgres) vs local mode differences. Use when working on crates/infigraph-core/src/multi/, adding a web framework's contract detection, or debugging group build/query/deps.
---

# Multi-repo groups

Core files: `crates/infigraph-core/src/multi/combined.rs` (~1550 lines — merging per-repo graphs), `cross_service.rs` (~2174 lines — contract extraction + cross-service edge detection), `mod.rs`, `bridge.rs`. This is one of the largest and least AST-driven subsystems in the codebase — most of the logic here is string/regex heuristics, not tree-sitter queries.

## Contract extraction — 3-tier priority, all heuristic

Not AST-based. In order:
1. **Route-kind symbols** — if the route detection pass (`routes/`) already found and tagged a `Route`-kind symbol, use it directly (highest confidence, AST-derived).
2. **Decorator/docstring regex parsing**, per-framework — pattern-matches things like FastAPI/Flask decorator text or docstrings for path/method info when no `Route` symbol exists.
3. **Router-prefix scraping from raw source** — regex-scrapes patterns like `APIRouter(prefix="/api/v1")` directly out of source text as a last resort, string-heuristic with no AST backing.

If you're adding contract detection for a new framework, decide which tier it needs: if the framework is already covered by `routes/` (see the route-coverage table in root README), tier 1 already covers it — don't duplicate with a tier-2 regex. Only add tier-2/3 patterns for frameworks/idioms `routes/` doesn't reach.

## Cross-service dependency detection (`detect_cross_service_deps` in `cross_service.rs`)

- Builds a route lookup with **exact-path + wildcard-prefix matching** (`/a/b/*`).
- Strips f-string/template interpolation from dynamic URLs before matching (`strip_fstring_prefix`) — so `f"/users/{user_id}"` matches a `/users/*` route.
- Resolves `CALLS_SERVICE` edges with **self-match suppression** (a service's own routes don't link to itself) and **method-preference fallback** (prefers exact HTTP-method match, falls back if none).

If cross-service edges seem missing or wrong: check whether the calling code's URL construction pattern is covered by `strip_fstring_prefix` or an equivalent — string-templated URLs that don't match known interpolation syntax will fail to match even if the route exists.

## Combined-graph merge pipeline (`combined.rs`)

Per-repo Kùzu DBs are merged via **Parquet, not row-by-row inserts**: `COPY TO` (export each repo's tables to Parquet) → column-prefixing (`[repo]::` prefix applied to IDs) → `COPY FROM` (bulk-load into the combined graph). This is a deliberate performance choice — row-by-row `CREATE` across dozens of repos would be far slower.

**ID-prefixing rules differ between node tables and edge tables** — Symbol/Module-style node tables get their primary ID column prefixed with `[repo]::`; edge tables need both endpoints' IDs rewritten consistently, not just one side. If you add a new node or edge type that needs to participate in the combined graph, follow the existing prefixing convention exactly — a half-prefixed edge silently produces dangling references in the combined DB, not an error.

## Remote mode (`--features remote`, `INFIGRAPH_BACKEND=neo4j`)

Materially different code path, gated by `is_remote_mode()`:
- Graph store → Neo4j (concurrent writes; local mode is Kùzu, single-writer via the flock guard described in the root `CLAUDE.md`).
- Registry/sessions → Postgres (persistent across container restarts; local mode uses a JSON registry file).
- `group build` indexes repos **in parallel** via rayon in remote mode (impossible in local mode since Kùzu is sequential/single-writer).
- Namespace prefixing prevents symbol-path collisions across repos (`svc-auth/src/main.rs` vs `svc-gateway/src/main.rs`).

**If you add a new feature to `multi/`, check whether it needs an `is_remote_mode()` branch** — forgetting to gate a feature means it either silently no-ops in remote mode or (worse) assumes single-writer Kùzu semantics that don't hold against concurrent Neo4j writes. See `docs/REMOTE-MULTI-REPO.md` for the full remote-mode design.
