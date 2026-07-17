# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Infigraph is an AST-powered code intelligence engine: it indexes codebases (62 languages) into a persistent embedded graph database (LadybugDB, `lbug` — the Kuzu successor) with Cypher queries, hybrid BM25+semantic search, cross-file call resolution, and 82 MCP tools for AI coding agents. Rust workspace, zero LLM dependency, fully local/offline.

## Build, test, lint

```bash
# Build (release, the two binaries that matter for manual testing)
cargo build --release -p infigraph-cli -p infigraph-mcp

# Full test suite (what CI runs)
cargo test --all

# Single test
cargo test -p infigraph-core some_test_name
cargo test -p infigraph-mcp --test watcher_concurrency test_second_watch_project_call_declines_when_already_watching

# Format + lint (both enforced in CI, run before pushing)
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

System dependency: `cmake` (macOS: `brew install cmake`, Linux: `apt install cmake`) — required to build the embedded graph DB.

CI (`.github/workflows/ci.yml`) runs `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all` across macOS/Linux/Windows. Match this locally before pushing.

### Release process

`./release.sh v1.0.0` — builds `aarch64-apple-darwin` targets, ad-hoc codesigns both binaries, packages `infigraph`/`infigraph-mcp`/`models/` into `infigraph-<target>.tar.gz`, and creates/uploads a GitHub release. Requires `gh` CLI authenticated to `github.com` (not an enterprise host — see the `review-pr-against-issue` skill for the same gh-host gotcha in a different context), `cmake`, Rust toolchain. Full details in README's [Releasing a new version](README.md#releasing-a-new-version-maintainers) section.

### Windows-specific build quirks (see root `Cargo.toml`)
- `lbug` (the graph DB) produces a static lib that exceeds the 4GB PE32 limit in debug mode on Windows — worked around via `[profile.dev.package.lbug] opt-level = 2`.
- `esaxx-rs` hardcodes `/MT` static CRT which conflicts with `lbug`'s `/MD` — patched via `[patch.crates-io]` to `patches/esaxx-rs`.
- Cross-compiling macOS→Windows is unsupported (lbug needs C++20 `<format>`, GCC 13+, but cross Docker images ship GCC 9). Build natively on Windows.

## Architecture

### Crate layout (`crates/`)

- **`infigraph-core`** — the engine. Everything downstream (CLI, MCP, docs) depends on this. Key submodules:
  - `model/` — `Symbol`, `Relation`, `FileExtraction` — the core data types every language extractor produces
  - `lang/` — `LanguageRegistry`, `LanguagePack` abstraction that both tree-sitter and grammar-plugin backends implement
  - `extract/` — AST → `Symbol`/`Relation` extraction (tree-sitter query execution + `.scm` capture handling)
  - `graph/` — LadybugDB store, schema, Cypher query execution
  - `search/` — BM25 + Model2Vec hybrid search, HNSW vector index
  - `resolve/` — cross-file call resolution (import-aware; this is what turns "a function calls `foo()`" into an edge to the actual `foo` definition, possibly in another file)
  - `multi/` — multi-repo registry, groups, cross-service HTTP contract extraction, `CALLS_SERVICE` edge linking (`combined.rs`, `mod.rs`)
  - `watch/` — file watcher (auto-starts post-index, notify-based, canonicalizes roots — FSEvents on macOS delivers absolute symlink-resolved paths, so a non-canonical root breaks `strip_prefix` silently)
  - `scip/` — SCIP index import for compiler-grade enrichment (overlays tree-sitter-derived symbols with precise type/reference info)
  - `taint/`, `concerns/`, `reflection/`, `config/` — dataflow/security/framework-pattern analysis passes, all operating on the same graph
- **`infigraph-languages`** — 59 tree-sitter language packs, each a pair of `.scm` query files (`entities.scm` for symbols, `relations.scm` for calls/imports/inheritance). Adding tree-sitter language support means adding queries here, not Rust code.
- **`infigraph-grammar-plugin`** — runtime ANTLR grammar plugin system for languages without tree-sitter grammars. Spawns a persistent JVM subprocess (`driver/infigraph-driver.jar`) that loads user-dropped `.g4` grammars in interpreter mode (no codegen/compilation) and returns JSON parse trees over stdin/stdout; Rust walks the tree per `plugin.toml` entity/relation rules. Both this and tree-sitter emit the same `Symbol`/`Relation` types, so everything downstream is backend-agnostic.
- **`infigraph-pipeline-plugin`** — analogous runtime-plugin system for data-pipeline metadata (dbt, Airflow, etc.), subprocess + JSON IPC, separate from the code graph but feeds the same `PipelineCore` table for cross-plugin dependency/impact queries.
- **`infigraph-docs`** — document indexing (PDF/DOCX/PPTX/HTML/Markdown), separate index feeding the same hybrid search pipeline.
- **`infigraph-confluence`** — Confluence wiki BFS crawler with incremental sync, feeds into the doc index.
- **`infigraph-cli`** — the `infigraph` binary, 50+ subcommands.
- **`infigraph-mcp`** — the `infigraph-mcp` binary: 82-tool MCP server (stdio and/or HTTP transport) + embedded web UI (vis.js graph explorer, served on the same process at `localhost:9749`). Cross-session context persistence (`save_session`/`get_latest_session`) lives here in `src/tools/session.rs`, backed by `infigraph-core/src/meta/` for the remote/Postgres path — there is no `session/` submodule in `infigraph-core` itself.
- **`lsp-to-scip`** — generic bridge binary: spawns any LSP server and emits a SCIP index, for languages with no dedicated SCIP indexer.
- **`crates/tree-sitter-vb6`** — vendored grammar crate for VB6.

### Data flow

```
source files → tree-sitter/grammar-plugin extraction → Symbol/Relation
  → cross-file resolution (resolve/) → LadybugDB graph
  → [optional] SCIP import overlays compiler-grade symbols/types
  → search index (BM25 + Model2Vec embeddings) + HNSW vector index
  → 82 MCP tools query the graph/search index directly (no LLM in the loop)
```

Both local mode (LadybugDB, single repo) and remote mode (`--features remote`: Neo4j for the graph + Postgres for registry/sessions, namespace-prefixed paths for 30+ repo multi-tenant indexing) speak the same `Symbol`/`Relation`/edge schema — see `docs/REMOTE-MULTI-REPO.md` for what specifically changes in remote mode (concurrent writes, parallel group indexing since Kùzu/lbug is single-writer/sequential).

### Cross-cutting invariants worth knowing before editing

- **Write-lock safety**: lbug is single-writer. All write paths (indexing, SCIP import, structured ingestion, cross-service linking, resolve) take an advisory `flock` RAII guard that releases on drop or crash. If you add a new write path, it needs this lock too — grep existing callers in `graph/` before assuming a new write site is safe.
- **Gitignore-aware indexing**: respects `.gitignore` and `.infigraphignore` via the `ignore` crate — don't hand-roll path filtering elsewhere.
- **Custom edge types are extensible per-language-plugin** (e.g. `DECORATED_BY`, `SPAWNS`) — the graph schema isn't a fixed enum; check `graph/schema` before assuming an edge kind list is exhaustive.
- **Watch-lock file** (`.infigraph/watch.lock`) is the cross-process dedup mechanism preventing two CLI/MCP processes from double-watching the same project — any code path that starts a watcher must acquire this lock for the watcher's lifetime, not just peek at it.
- **Route/decorator extraction** uses two different mechanisms depending on language syntax: tree-sitter query capture (`entities.scm`) for languages where decorators are syntactic wrappers (Python `decorated_definition`), vs. AST sibling scan (`extract/entities.rs`, `ATTR_KINDS` const) for languages where attributes are preceding sibling nodes (Rust `attribute_item`, C# `attribute_list`). Adding route support for a new language means picking the right one of these two paths.
- **Fixed-path scratch files aren't run-unique**: e.g. `.infigraph/scip-enrich.log` and `.infigraph/scip-tmp/<indexer>.scip` use no PID/run-id, so two concurrent `infigraph index` runs on the same repo can race on them (log truncation, output clobbering). Keep this in mind if you add new background/scratch file paths — scope them per-PID or per-run-id, or route them through the existing write-lock.

### Test fixtures

`tests/fixtures/microservices/` — three real microservice repos (Python/Flask, TypeScript/Express, Rust/Actix) used for route detection and cross-service dependency tests. Prefer extending these over inventing new synthetic fixtures when testing cross-file/cross-service behavior.
