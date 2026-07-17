---
name: adding-a-language
description: Add support for a new programming language to Infigraph — tree-sitter query path vs ANTLR grammar-plugin path, entities.scm/relations.scm conventions, and registration. Use whenever asked to add language support, write entities.scm/relations.scm, or add a grammar plugin.
---

# Adding a new language

## Decision point: tree-sitter vs grammar plugin

If a `tree-sitter-<lang>` crate exists on crates.io, use the **tree-sitter path** (write query files, no Rust compilation of grammar). If no tree-sitter grammar exists for the language, use the **ANTLR grammar-plugin path** (`.g4` grammars + Java extractor, runs via a JVM subprocess, no Rust recompilation either — see `GRAMMAR_PLUGINS.md` for the full walkthrough).

## Tree-sitter path

Directory: `crates/infigraph-languages/languages/<lang>/`

- **`lang.toml`** — `[language] name`, `extensions`, `grammar = "tree-sitter-<lang>"`, plus optional `[[custom_edges]]` blocks for language-specific edge kinds (e.g. Python declares `name = "DECORATED_BY"`, `capture = "decorates"` to promote a capture pair into a graph edge type beyond the built-in CALLS/IMPORTS/INHERITS set).
- **`entities.scm`** — tree-sitter queries capturing symbols. Naming convention: `@<kind>.name` for the identifier, `@<kind>.def` for the whole node, optional `@<kind>.docstring` / `@<kind>.decorator`. Kinds in use: `func`, `class`, `method`, `test`, `var`, `route`. Use `#match?` predicates for things like test-function detection (`test_` prefix) or route decorator patterns (Django `path()`/`re_path()`/`url()` calls).
- **`relations.scm`** — call/import/inherit queries. Convention: `@call.func` + `@call.site` for calls; `@call.receiver` for the object in `obj.method()` (this feeds receiver-aware resolution — see the `cross-file-call-resolution` skill); `@import.module` for imports; `@inherit.child`/`@inherit.parent` for class bases; custom capture pairs (e.g. `@decorates.target`/`@decorates.source`) matching any `[[custom_edges]]` declared in `lang.toml`.

Look at `crates/infigraph-languages/languages/python/` (has decorators, routes, receiver-aware calls) and `crates/infigraph-languages/languages/rust/` (simpler — no decorators/routes, just `func`/`class`/`method`/`var`) as reference points depending on how much of the language's syntax you need to cover.

**Registration**: add your language's `_pack()` function to `crates/infigraph-languages/src/lib.rs::bundled_registry()`. Query files are embedded at compile time via `include_str!`. A pack that fails to load is skipped with a warning (`eprintln!("warning: failed to load ... language pack: {e}")`), not a hard error — don't rely on a broken pack blocking startup.

## Grammar-plugin path (ANTLR)

Plugin = `.g4` grammar files + a Java extractor class + `plugin.toml`, discovered from a directory. Key `plugin.toml` fields: `name`, `extensions`, `entry_rule`, `lexer`, `parser`, `extractor` (class name or `.java` path), optional `preprocessor = "c"` (enables JCPP for `#ifdef`/`#include`), `emit_referenced_form_imports`, `pipe_strings`.

Extractor implements `processRule(ruleName, tree, tokens, ctx)`, returning `true` if the rule opens a scope. Use `ctx.pushSymbol(...)`, `ctx.pushRelation(...)`, `ctx.pushFormQualifiedRelation(...)` (for cross-file `FORMNAME::field` style targets) to emit data.

Discovery/loading (`crates/infigraph-grammar-plugin/src/`):
- `lib.rs::register_grammar_plugins()` searches, in order: bundled dir next to the binary → `~/.infigraph/grammars/` → `<project>/grammars/`. Spawns **one shared JVM** for all plugins (needs `infigraph-driver.jar`, located via `find_driver_jar()` — next to binary, `../driver/`, or `INFIGRAPH_DRIVER_JAR` env var). No jar found → silently skipped unless `INFIGRAPH_DEBUG` is set.
- `plugin.rs::discover_plugins()` walks subdirectories, skips (with a warning) any plugin whose `.g4` files are missing.
- `driver.rs::GrammarDriver` talks to the JVM over stdin/stdout as line-delimited JSON commands (`load`, `set_extractor`, `extract`, `parse`, `shutdown`).

**Registration order matters**: `crates/infigraph-cli/src/main.rs::full_registry()` registers tree-sitter packs first, then grammar plugins second. Both write into the same `by_extension` map in `LanguageRegistry`, so a grammar plugin claiming an extension already owned by a tree-sitter pack **overwrites it** (last-registered wins) — this is how you'd override tree-sitter handling for an extension with a grammar plugin if ever needed.

## After adding: test

Index `tests/fixtures/microservices/` (or a new fixture) and confirm symbols/relations appear via `get_symbols_in_file` / `search`. If you add route detection, check the language against the coverage table in README's "Contributing: Route & Decorator Extraction" section.
