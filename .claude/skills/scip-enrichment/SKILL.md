---
name: scip-enrichment
description: How SCIP (compiler-grade) enrichment works end to end — indexer selection/download, running the indexer, importing the .scip file, and how it merges with tree-sitter-derived symbols and CALLS edges. Use when debugging SCIP import, adding a new SCIP indexer, or working on lsp-to-scip.
---

# SCIP enrichment pipeline

## End-to-end flow

1. **Language detection**: after tree-sitter extraction, `result.extractions` gives the detected language set.
2. **Indexer selection**: `crates/infigraph-cli/src/scip_download.rs::indexers_for_languages(detected)` matches against a static `CATALOG` table. Each entry has `lang_tags`, `binary_name`, `scip_args`, `output_flag`, and a `DownloadStrategy` (`GithubRelease`, `NpmInstall`, `DotnetTool`, `ComposerInstall`, `DartPubInstall`). Examples: TypeScript/JS → `scip-typescript` (npm), Python → `scip-python` (npm), Rust → `rust-analyzer` (GitHub release, OS/arch-mapped asset), Java/Kotlin/Scala → `scip-java` (needs a JVM), Go → `scip-go`, Ruby → `scip-ruby`, C#/F# → `scip-dotnet`, C/C++ → `scip-clang`, Dart → `scip-dart`, PHP → `scip-php`.
3. **Download/caching**: `ensure_indexer(indexer)` checks `PATH` first, then a cache dir (`~/.infigraph/bin/` on Unix, platform data dir on Windows), else installs via the matched strategy. Portable runtimes (Node 22.16.0, JRE 21/Temurin, .NET SDK 8.0.412, Dart 3.12.1, PHP 8.4.21) self-provision under `~/.infigraph/{node,java,dotnet,dart,php}/` with `.version` marker files — a version bump wipes and re-downloads. Downloads try `ureq` first, fall back to `curl`.
4. **Running the indexer** — two paths:
   - Background/detached (the normal post-`infigraph index` path): `spawn_scip_child_process()` respawns the current binary as `scip-enrich <languages>` (see the discarded-`spawn()`-result gap noted in the `review-pr-against-issue` skill / issue #11 history), logs to `.infigraph/scip-enrich.log`.
   - Foreground: `auto_scip()` runs indexers in parallel via `std::thread::scope`, with `PATH` augmented by `extra_runtime_paths()` so portable runtimes are found.
5. **Importing**: `infigraph_core::scip::import_scip_index(&scip_out, store, Some(root))` in `crates/infigraph-core/src/scip/mod.rs`:
   - Parses `index.scip` via `protobuf`/`scip::types::Index`, builds a `scip_sym_to_file_name` map.
   - **Pass 1** enriches existing tree-sitter symbols matched by `(file, name)` — sets `start_line`/`end_line`/`docstring`. If no tree-sitter symbol exists at that `(file, name)`, **adds a new `Symbol`** node with `language: "scip"`. Both cases use bulk Parquet `COPY` with UNWIND fallback.
   - **Pass 2** builds CALLS edges from SCIP reference occurrences: finds the enclosing container by span containment, the target via `scip_sym_to_file_name`, and records corrections to the learned-resolution cache when this disagrees with an existing tree-sitter edge (see `cross-file-call-resolution` skill).
6. **Merge semantics — augment, don't replace**: existing Symbol nodes get enriched in place; missing ones are added as new `language: "scip"` symbols; CALLS edges are added additively (deduped via a `seen_edges` set) alongside pre-existing tree-sitter edges — same `CALLS` relation kind, no separate SCIP-edge type. Any resolution discrepancy is captured in the learned-pattern cache, not by deleting the old edge.

## Generic LSP bridge (`crates/lsp-to-scip`)

For languages without a dedicated SCIP indexer. CLI: `lsp-to-scip --server "<lsp command>" --root <path> --lang <lang> --out index.scip`.

- Spawns the LSP server as a child process, does `initialize`/`initialized`, opens every matching source file (`did_open`).
- Requests `textDocument/documentSymbol` per file to build SCIP `SymbolInformation` + definition occurrences (symbol id via `symbol_string(rel_path, name, kind)`).
- For files with under 500 symbols, also requests `textDocument/references` per symbol to add same-file reference occurrences.
- **Cross-file references are not captured by this bridge** — only same-file reference edges. Cross-file linking for these languages depends entirely on tree-sitter's own resolution (`resolve/`) plus whatever the SCIP import's Pass 1/2 can still do with same-file-only reference data.
- Used for: C/C++ (clangd), Zig (zls), Swift (sourcekit-lsp), Elixir (elixir-ls), Dart, Haskell, F#, Clojure, Erlang, and any language with an LSP server.

## Adding a new SCIP indexer

Add a `CATALOG` entry in `scip_download.rs` with the right `DownloadStrategy`. If it needs a runtime not already self-provisioned (Node/JRE/.NET/Dart/PHP), you'll need to extend the runtime-provisioning code too — check `ensure_runtime_for()` before assuming a new runtime type is a one-line change.

## Debugging a failed or silent SCIP enrichment

- Check `.infigraph/scip-enrich.log` first (background child's stderr) — look for `"Auto-SCIP: ..."` prefixed messages: download failures, extraction failures, `COPY ... failed` (falls back to UNWIND), `corrections_learned` counts.
- If enrichment appears to silently never run at all: confirm the respawned child actually launched and exited cleanly — as of the historical #11 bug, an arg-mismatch could kill the child instantly with only the log file as evidence and a success-looking "starting in background" message printed regardless. Check the log's mtime, not just its existence.
- `should_run_indexer()` gates some indexers on project-specific preconditions (e.g. `scip-clang` needs `compile_commands.json`, `scip-ruby` needs a `.gemspec`) — a skipped indexer isn't a bug, check for the `"Auto-SCIP: skipping ..."` message before assuming something's broken.
