# Design: FastAPI `Depends()` / `add_middleware()` / `call_next()` call-graph visibility

Status: design discussion, not scheduled. Tracked as AIF3X-331 #16.

## Problem

The AIF3X-331 evaluation report (external MCP eval against `llm-execution-svc`)
flagged this as the top P0 alongside the CALLS-edge bugs fixed in #12-#15:
InfiGraph can find FastAPI middleware and dependency-injection *symbols* via
`search`/`semantic_search`, but the call graph cannot answer "what runs before
this router handler?" — `trace_callers` on a `Depends()`-registered function
or an `add_middleware()`-registered middleware returns only unit-test callers
or nothing, even though the registration site is real, present in source, and
cited by the report at exact file:line locations (`__init__.py:591,836,840,
842,877,829,562`).

This doc scopes what's actually tractable to fix, split from what genuinely
needs new modeling — the prior triage session pushed back on the report's
uniform-P0 framing, and the four sub-problems bundled under this one ticket
turn out to have very different difficulty.

## Ground truth: four distinct node shapes, verified via tree-sitter

Confirmed by parsing each pattern directly (not guessed from the report's
prose):

1. **`Depends()` in a handler's parameter default**
   `def handler(x=Depends(validate_fn)): ...`
   Parses as `(default_parameter value: (call function: (identifier) "Depends")
   arguments: (argument_list (identifier) "validate_fn")))`.

2. **`Depends()` in a router's `dependencies=[...]` kwarg** — *the report's
   actual cited case* (`__init__.py:836,840,842`, `APIRouter`/
   `include_router`, not handler files):
   `router = APIRouter(dependencies=[Depends(validate_fn), Depends(other_fn)])`
   `app.include_router(chat_router, dependencies=[Depends(validate_fn)])`
   Parses as a `keyword_argument name: "dependencies" value: (list (call
   function: (identifier) "Depends" arguments: (argument_list (identifier)
   "validate_fn")) ...)`.

3. **`add_middleware()`** — two sub-shapes:
   - Positional class only: `app.add_middleware(RawContextMiddleware)`
   - `dispatch=` kwarg pointing at a plain function:
     `app.add_middleware(BaseHTTPMiddleware, dispatch=v3_logging_context_middleware)`
   Both parse as `(call function: (attribute object: "app" attribute:
   "add_middleware") arguments: (argument_list ...))`.

4. **`call_next(request)`** inside a middleware function body — the runtime
   dispatch call that hands control to "whatever's next in the stack."

For 1-3, the target symbol (`validate_fn`, `v3_logging_context_middleware`,
`RawContextMiddleware`) sits as a **plain identifier in argument position**
of a framework call (`Depends`, `add_middleware`) — exactly the same
positional relationship the existing `python/relations.scm` call patterns
already parse for ordinary calls, just one level deeper (argument, not
callee). Confirmed via `mcp__infigraph__query_graph` against a live fixture:
the *decorator* on a route handler is already captured (as docstring text),
but nothing in the current query captures `Depends`'s or `add_middleware`'s
**arguments** — only their callee name, which isn't a real user symbol and
resolves to nothing.

## Why this is NOT one uniform problem

**Shapes 1-3 are statically tractable — same difficulty class as #13/#15.**
The dependency function is a named, resolvable symbol sitting in a fixed
argument position of a recognizable framework call. This requires:
- New `.scm` query patterns capturing the *argument* identifier inside
  `Depends(...)` calls (both parameter-default and `dependencies=[...]`
  list positions) and inside `add_middleware(...)` (both positional class
  arg and `dispatch=` kwarg).
- A new relation kind (e.g. `DependsOn` for 1-2, `RegistersMiddleware` for 3)
  distinct from `Calls` — these are registrations, not invocations, and
  conflating them with `Calls` would corrupt existing call-graph semantics
  (a handler doesn't "call" its dependency at parse time; the framework
  calls it at request time).
- Resolution reuses the existing cross-file symbol-map (name → candidates)
  already built in `resolve/calls.rs`, but is not a drop-in: that function's
  per-relation loop currently opens with `if rel.kind != RelationKind::Calls
  { continue; }`, actively skipping every non-`Calls` relation. Routing the
  new kinds through requires extending that filter, plus unioning the new
  edge kinds into the `callers_of`/`trace_callers` query path (see Phasing
  step 2) — no new resolution *algorithm*, but real wiring, not free.

**Shape 4 (`call_next`) is genuinely hard and shouldn't be bundled with 1-3.**
`call_next(request)` dispatches to "the next middleware in the chain," but
that ordering isn't present at the call site at all — it's implied by the
*sequence* of `add_middleware()` registration calls elsewhere (in
`create_app`, typically), and middleware order in Starlette/FastAPI is
LIFO relative to registration order. Modeling this requires:
- Reconstructing registration order across possibly-multiple `add_middleware`
  calls (stateful, cross-statement, cross-file if middleware is registered
  conditionally or via a helper function) — Starlette/FastAPI's actual
  ordering semantics (commonly described as LIFO relative to registration
  order) should be verified against the framework source before being relied
  on, rather than assumed from memory.
- Deciding what `call_next` "points to" when the chain is dynamic (feature
  flags, conditional registration) — there may not be one static answer.

Recommendation: ship 1-3 as a scoped fix; treat 4 as out of scope for now,
or approximate it with a single coarse edge (`middleware -[NEXT_IN_CHAIN]->
router-dispatch`) rather than trying to resolve the exact next handler.

## Proposed edge model

| Relation kind         | Source                          | Target                     | Captures |
|---|---|---|---|
| `DependsOn`            | route handler or router/app var | dependency function        | shapes 1, 2 |
| `RegistersMiddleware`  | `app` variable's owning module   | middleware class/dispatch fn | shape 3 |

Both are additive — existing `Calls`/`Imports`/`Inherits` semantics are
unchanged. The `DependsOn` source differs by shape: for shape 1
(parameter-default `Depends()`) it's the handler's own symbol — confirmed via
`query_graph` that FastAPI handlers are `Function`-kind with the decorator
captured as docstring text (`extract_contracts`'s decorated-function path),
**not** `Route`-kind (`Route`-kind is reserved for call-expression frameworks
like Express/Gin/Django). For shape 2 (`dependencies=[...]`) the source is
the router/app variable or the `include_router`/`APIRouter(...)` call site
itself.

## Success criteria (from the report's own repeatable test battery)

Anchoring to the report's items 6 and 7 directly, since those are the two
`trace_callers` cases with exact ground-truth citations:

- Item 6: `trace_callers(symbol_id=...::validate_request_headers)` →
  currently "2 unit tests only"; should additionally surface the
  `APIRouter(dependencies=[...])`/`include_router(..., dependencies=[...])`
  registration site(s) via the new `DependsOn` edge.
- Item 7: `trace_callers(symbol_id=...::v3_logging_context_middleware)` →
  currently "6 unit tests only"; should additionally surface the
  `app.add_middleware(..., dispatch=...)` registration site via the new
  `RegistersMiddleware` edge.

Item 8 (`trace_callees(process_chat_request) expect do_input_risk_screening`)
is unrelated to this doc — that's the CALLS-edge gap already fixed in #14/#15.

`detect_cross_cutting` (currently pattern-matches only decorator-shaped
concerns like `@lru_cache`/`@field_validator`, confirmed by reading
`concerns/mod.rs`) is a natural second consumer of the new edge kinds once
they exist, but extending it is separate follow-on work, not part of this
doc's scope.

## Phasing

1. `.scm` patterns for shapes 1-3 + new relation kinds + resolution wiring
   (reuses existing symbol-map/cross-file resolution, no new algorithm).
2. Wire `DependsOn`/`RegistersMiddleware` into `trace_callers` output so
   registration sites appear alongside real callers, not just literal CALLS
   edges — needs a small `GraphQuery`/`callers_of`-adjacent change to union
   the new edge kinds in, not just `CALLS`.
3. (Separate, lower priority) Extend `detect_cross_cutting` to classify
   these as cross-cutting concerns.
4. (Out of scope / open question) `call_next` chain-ordering — revisit only
   if a concrete need shows up; approximate rather than fully resolve if
   pursued.

## Non-goals

- Runtime request-flow tracing (this is static analysis; no attempt to model
  actual dispatch order at runtime).
- Full `call_next` chain resolution (see Shape 4 above).
- Changing `Calls` semantics — `DependsOn`/`RegistersMiddleware` are new,
  additive edge kinds, not repurposed `Calls` edges.
