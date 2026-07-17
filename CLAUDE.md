# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Working convention

For any non-trivial analysis, review, or investigation (not routine edits), end with a **confidence** (High/Medium/Low) and a **recommendation** (e.g. merge/hold/needs more info) so the result is decision-ready.

## What this is

Infigraph is an AST-powered code intelligence engine: indexes codebases (62 languages) into a persistent embedded graph database (LadybugDB) with Cypher queries, hybrid search, cross-file call resolution, and MCP tools for AI coding agents. Rust workspace, zero LLM dependency, fully local/offline.

## Build, test, lint

```bash
cargo build --release -p infigraph-cli -p infigraph-mcp
cargo test --all
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

Requires `cmake`. CI runs fmt-check, clippy, and the full test suite across macOS/Linux/Windows — match locally before pushing.

**Change-safety workflow**: check blast radius before editing a shared/foundational path (e.g. via cross-file reference lookups) — a change in `infigraph-core` can silently break the CLI/MCP crates on top of it. Run targeted tests for the area you touched first, then `cargo test --all` before considering it done — this repo has real process-level integration tests (e.g. under `infigraph-mcp/tests/`), not just unit tests, so `--all` matters. The pre-commit hook enforces fmt/clippy across the whole workspace, not just changed files — pre-existing unrelated drift elsewhere can block an otherwise-clean change; worth checking `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` once at the start of a session to know your baseline.

Release process: `./release.sh <version>` (see README's release section). Windows has a few build quirks around the graph DB's static-lib size and CRT linkage — see root `Cargo.toml` comments.

## Architecture

### Crate layout (`crates/`)

- **`infigraph-core`** — the engine: models, language registry, AST extraction, graph storage, search, cross-file resolution, multi-repo/groups, file watching, SCIP enrichment, and the analysis passes (taint, concerns, reflection, config binding).
- **`infigraph-languages`** — tree-sitter language packs (query files, not Rust code).
- **`infigraph-grammar-plugin`** — runtime ANTLR-based plugin system for languages without a tree-sitter grammar.
- **`infigraph-pipeline-plugin`** — runtime plugin system for data-pipeline metadata extraction.
- **`infigraph-docs`** / **`infigraph-confluence`** — document and wiki indexing, feeding the same search pipeline.
- **`infigraph-cli`** — the `infigraph` binary.
- **`infigraph-mcp`** — the MCP server binary, plus the embedded web UI and session persistence.
- **`lsp-to-scip`** — generic LSP→SCIP bridge for languages without a dedicated SCIP indexer.

### Data flow

Source → AST extraction → cross-file resolution → graph → optional SCIP enrichment → search index. Both local (embedded graph DB) and remote (`--features remote`: Neo4j + Postgres, multi-repo) modes share the same schema — see `docs/REMOTE-MULTI-REPO.md`.

### Cross-cutting invariants worth knowing before editing

- The graph DB is single-writer — write paths take an advisory lock; a new write path needs one too.
- Indexing respects `.gitignore`/`.infigraphignore` — don't hand-roll path filtering.
- The graph's edge/node schema is extensible per-language-plugin, not a fixed enum.
- File-watching uses a lock file for cross-process dedup — anything that starts a watcher must hold it for the watcher's lifetime.
- Route/decorator extraction differs by language syntax (query-based vs. AST-sibling-scan) — check both before assuming a language isn't covered.
- Background scratch files (SCIP enrichment log/tmp) use fixed, non-run-unique paths — concurrent `infigraph index` runs can race on them.

For deeper detail on any of these, see the matching skill/rule (adding a language, cross-file call resolution, SCIP enrichment, multi-repo groups, taint analysis, debugging indexing/watch issues).

### Test fixtures

`tests/fixtures/microservices/` — real microservice repos (Python/Flask, TypeScript/Express, Rust/Actix) for route and cross-service tests. Prefer extending these over new synthetic fixtures.
