---
name: debugging-indexing-watch
description: Systematic triage for "index seems stale", "watcher not firing", "reindex not happening", or FD-leak-style watch reports. Use when debugging file-watcher or incremental-reindex issues.
---

# Debugging indexing/watch issues

## How the watcher works

Events batch over a short debounce window before triggering reindex. Ignored paths (VCS/build/dependency dirs, dotfiles) never enter the batch. The watch root is canonicalized before comparison, since some backends (FSEvents on macOS) deliver symlink-resolved absolute paths regardless of how the root was specified.

A cross-process lock file prevents two CLI/MCP processes from double-watching the same project — whichever process starts a watcher must hold this lock for the watcher's lifetime.

## Concrete checklist

1. **Is a watcher running?** Check the lock file's holder, or use the MCP watch-status tool.
2. **Why isn't a changed file triggering reindex?** In order of likelihood: unrecognized file extension (silently dropped, no log line), file under an ignored directory, a batch/reindex failure logged by the watcher, or — if cross-file calls changed and auto-resolve is off — this is expected behavior requiring a manual reindex.
3. **Watcher stopped entirely?** The underlying OS watch can crash and retry a bounded number of times before giving up permanently — that requires a manual restart, not just waiting.
4. **SCIP enrichment issues** are a separate subsystem — see that skill, not this one.
5. **FD-leak reports (macOS)** — check whether a kqueue-based watch backend is enabled anywhere in the dependency tree via feature unification; FSEvents (the default) should be used instead.
