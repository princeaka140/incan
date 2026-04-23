# RFC 069: `list.repeat` Helper for Fixed-Length Initialization

- **Status:** Draft
- **Created:** 2026-04-17
- **Author(s):** Danny Meijer (@dannymeijer)
- **Related:**
    - RFC 030 (std collections baseline)
- **Issue:** https://github.com/dannys-code-corner/incan/issues/385
- **RFC PR:** —
- **Written against:** v0.2
- **Shipped in:** —

## Summary

This RFC proposes one standard-library helper, `list.repeat[T](value: T, count: int) -> list[T]`, to create fixed-length lists initialized with the same value, so authors can express intent directly without manual append loops or sentinel-map boilerplate.

## Motivation

In current Incan code, fixed-length initialization often appears as repeated patterns like “create empty list, iterate N times, append default/sentinel value.” This is mechanically correct but noisy, harder to scan, and easy to get subtly wrong in graph and indexing code. Comprehensions reduce some noise, but the core intent is still “repeat this value N times,” and that intent deserves a dedicated API.

This is also a practical interop concern: Incan lowers to Rust, where this operation is first-class and widely used (for example, `vec![-1_i64; nodes.len()]`).

Python-first users are used to concise list initialization and should have an equally direct, explicit operation in Incan, without having to hand-write append loops for a common pattern. A standard helper keeps author intent clear while still matching Rust-side semantics (`Clone`-based element repetition).

## Goals

- Add one canonical stdlib helper for repeated list initialization.
- Improve readability for sentinel maps and pre-sized working buffers.
- Keep rollout lightweight by avoiding parser syntax changes.
- Make semantics explicit for count validation and element cloning.

## Non-Goals

- This RFC does not introduce new list literal syntax (for example Rust-style `vec![x; n]` sugar).
- This RFC does not add capacity-only allocation APIs.
- This RFC does not change existing list comprehension semantics.
- This RFC does not introduce deep-copy semantics beyond existing `Clone` behavior.

## Guide-level explanation

Use `list.repeat` when you want a list of length `N` where every element starts from the same value.

```incan
from std.collections import list

ids = list.repeat(-1, 8)
# ids == [-1, -1, -1, -1, -1, -1, -1, -1]
```

This is especially useful for id maps and temporary buffers:

```incan
from std.collections import list

def unmapped_ids[T](items: list[T]) -> list[int]:
    return list.repeat(-1, len(items))
```

For non-primitive values, the helper follows normal `Clone` semantics and requires cloneable element types.

## Reference-level explanation

The standard library must provide:

```incan
list.repeat[T with Clone](value: T, count: int) -> list[T]
```

Normative behavior:

- `count == 0` must return an empty list.
- `count > 0` must return a list of length `count`.
- Each returned element must be clone-derived from `value`.
- The function must not return aliased mutable storage that would violate normal value semantics expected from cloned elements.
- `count < 0` must raise a runtime `ValueError` with a stable message indicating `count` must be non-negative.

Type rules:

- `T` must satisfy `Clone`.
- The return type must be `list[T]` with no widening.

Complexity:

- Time complexity should be linear in `count`.
- Space complexity should be linear in `count`.

## Design details

### API location

The helper belongs in the std collections surface next to list operations.

### Why helper over syntax

This feature is a reusable collection operation, not a new language form. Keeping it in stdlib avoids parser/lowering churn and keeps behavior testable as a normal API.

### Clone semantics

`repeat` should behave like repeated `append(value.clone())` for each element. This keeps behavior aligned with existing trait expectations and avoids introducing custom duplication semantics.

### Error behavior

Negative `count` is a caller error and should fail at runtime with `ValueError` instead of silently returning empty output.

## Alternatives considered

- Add list literal repetition syntax (for example `[value; count]`): rejected for now because it expands language grammar for a problem that a stdlib API can solve cleanly.
- Keep using comprehensions (`[x for _ in range(n)]`): acceptable but less direct when the intent is repeated initialization rather than transformation.
- Keep explicit loops with append: most verbose and least declarative for this use-case.

## Drawbacks

- Adds another API surface that overlaps with patterns already expressible via comprehensions and loops.
- Requires `Clone` for repeated values, which can reject some types that authors might expect to repeat.
- Introduces one more runtime error path (`count < 0`) that callers must handle in dynamic inputs.

## Implementation architecture

Recommended implementation is in stdlib collections as a straightforward loop that appends `value.clone()` `count` times after validating the `count` guard, with unit tests for zero, positive, negative, and non-primitive cloneable values.

## Layers affected

- **Stdlib / Runtime (`incan_stdlib`)**: add `list.repeat` implementation and tests.
- **Typechecker / Symbol resolution**: no new language rules; standard generic-bound checking for exported stdlib signatures.
- **LSP / Tooling**: surface completion/hover docs for the new stdlib function.
- **Documentation**: update collections reference/how-to examples.

## Unresolved questions

- Should `list.repeat` live only under `std.collections.list`, or should there also be a top-level alias export for ergonomics?
- Should runtime diagnostics include the provided `count` value in the `ValueError` message, or keep a fixed message for snapshot stability?
- Should this RFC also define a companion API for repeated callable generation (for example `list.generate(count, fn(i) -> T)`), or keep that as a separate follow-on RFC?

<!-- Rename this section to "Design Decisions" once all questions have been resolved. An RFC cannot move from Draft to Planned until no unresolved questions remain. -->
