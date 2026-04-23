# RFC 070: Result Combinators for `Result[T, E]`

- **Status:** Draft
- **Created:** 2026-04-18
- **Author(s):** Danny Meijer (@dannymeijer)
- **Related:**
    - RFC 000 (core error handling and `Result` model)
- **Issue:** https://github.com/dannys-code-corner/incan/issues/386
- **RFC PR:** —
- **Written against:** v0.2
- **Shipped in:** —

## Summary

This RFC proposes adding first-class combinator methods to `Result[T, E]` in Incan, specifically `map`, `map_err`, `and_then`, and `or_else`, so fallible pipelines can be expressed directly without repetitive nested `match` scaffolding.

## Core model

1. `Result` stays explicit and typed; this RFC adds composition methods, not new error semantics.
2. Combinators are pure transformations over `Ok` and `Err` branches.
3. `?` remains the primary early-return mechanism; combinators complement `?` when values must be transformed in-line.

Mechanisms:

- `map`: transform `Ok(T)` into `Ok(U)` while preserving `Err(E)`.
- `map_err`: transform `Err(E)` into `Err(F)` while preserving `Ok(T)`.
- `and_then`: chain `Result`-returning operations on `Ok(T)`.
- `or_else`: recover or remap from `Err(E)` with a `Result`-returning function.

## Motivation

Current Incan code frequently repeats nested `match` expressions for straightforward `Result` branch transformations. This is correct but verbose, especially in backend adapters and interop-heavy code where planning, execution, and sink errors are repeatedly remapped to project-specific error types.

The language already supports `Result[T, E]` and `?`, but it lacks the compositional API surface users expect from Rust-shaped error handling. This gap leads to noisy code and pushes teams toward local helper functions for patterns that should be standard on `Result` itself.

## Goals

- Add canonical compositional methods on `Result[T, E]`.
- Reduce boilerplate for error/value transformation pipelines.
- Keep naming and semantics aligned with Rust conventions to reduce cognitive overhead.
- Preserve compatibility with existing `match` and `?` patterns.

## Non-Goals

- This RFC does not replace `match` as a control-flow construct.
- This RFC does not change `?` behavior.
- This RFC does not introduce exception-style implicit error propagation.
- This RFC does not define async-specific combinators in this slice.
- This RFC does not introduce new union error syntax beyond existing `Result` typing rules.

## Guide-level explanation

Use `Result` combinators when the operation is branch transformation rather than multi-step control flow.

```incan
def parse_int(raw: str) -> Result[int, str]:
    ...

def validate_positive(v: int) -> Result[int, str]:
    ...

def prefix_port_error(e: str) -> str:
    return f"invalid port: {e}"

def normalize_port(raw: str) -> Result[int, str]:
    return parse_int(raw).and_then(validate_positive).map_err(prefix_port_error)
```

Use `map` when only success changes:

```incan
def read_len(path: str) -> Result[int, str]:
    return read_text(path).map(len)
```

Use `or_else` when recovering from failure:

```incan
def read_default_text(_err: str) -> Result[str, str]:
    return Ok("default")

def read_with_default(path: str) -> Result[str, str]:
    return read_text(path).or_else(read_default_text)
```

`match` remains preferred when both branches require substantial logic or side effects.

## Reference-level explanation

Incan must expose the following methods on `Result[T, E]`:

```incan
def map[U](self, f: Fn[T, U]) -> Result[U, E]
def map_err[F](self, f: Fn[E, F]) -> Result[T, F]
def and_then[U](self, f: Fn[T, Result[U, E]]) -> Result[U, E]
def or_else[F](self, f: Fn[E, Result[T, F]]) -> Result[T, F]
```

Normative behavior:

- `map` must call `f` exactly once for `Ok(T)` and must not call `f` for `Err(E)`.
- `map_err` must call `f` exactly once for `Err(E)` and must not call `f` for `Ok(T)`.
- `and_then` must call `f` exactly once for `Ok(T)` and must propagate `Err(E)` unchanged.
- `or_else` must call `f` exactly once for `Err(E)` and must propagate `Ok(T)` unchanged.
- All combinators must preserve left-to-right evaluation order of receiver then closure.
- All combinators must remain type-safe under existing generic bound rules.

Diagnostics:

- When function argument arity or return type is incompatible with a combinator contract, the compiler must emit a type error at the combinator call site.

## Design details

### Naming

Names intentionally mirror Rust (`map`, `map_err`, `and_then`, `or_else`) for predictability and interop mental-model alignment.

### Closure requirements

Closure argument and return types must satisfy each combinator signature exactly under existing callable typing and generic substitution rules.

### Interaction with `?`

`?` remains the best tool for early exit in sequential code. Combinators should be used where local transformation or chaining is clearer than introducing a `match` or temporary bindings.

### Backward compatibility

This is additive API surface. Existing `Result` code using `match` and `?` remains valid.

## Alternatives considered

- Keep current style with helper functions + `match`: rejected because it duplicates a standard abstraction and fragments style.
- Add only `map_err`: rejected because it solves only one branch of the composition problem.
- Introduce pipeline syntax sugar instead of methods: rejected because method-based API is lower risk and easier to stage.
- Rename combinators to Python-style alternatives: rejected because semantic drift from Rust increases learning and documentation burden.

## Drawbacks

- Adds more methods to learn for beginners already learning `Result` and `?`.
- Can be overused in places where explicit `match` is clearer.
- Requires high-quality diagnostics to avoid confusing generic/closure mismatch errors.

## Implementation architecture

Recommended approach is to model combinators as stdlib-visible methods on `Result` with lowering that reuses existing match-like branch semantics, so runtime behavior remains explicit and unsurprising while source ergonomics improve.

## Layers affected

- **Typechecker / Symbol resolution**: method signatures on `Result`, callable compatibility checks, generic substitution for combinator methods.
- **IR Lowering**: lowering for combinator calls on `Result` receivers.
- **Emission**: stable Rust emission for combinator method calls with correct closure typing.
- **Stdlib / Runtime (`incan_stdlib`)**: `Result` method surface exposure where required by runtime bridge strategy.
- **LSP / Tooling**: completion, hover, and diagnostics for new `Result` methods.
- **Documentation**: error-handling reference and tutorial updates.

## Unresolved questions

- Should this RFC include `inspect` and `inspect_err` in the same slice, or keep them for a follow-on RFC?
- Should `and_then` and `or_else` support function values only in the first slice, or also accept callable objects immediately?
- Should method names remain strictly Rust-aligned, or should aliases be allowed in docs without adding language-level synonyms?

<!-- Rename this section to "Design Decisions" once all questions have been resolved. An RFC cannot move from Draft to Planned until no unresolved questions remain. -->
