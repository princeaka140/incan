# RFC 014: Error Handling in Generated Rust Code

**Status:** Draft  
**Created:** 2025-12-01  
**Author(s):** Danny Meijer (@danny-meijer)  
**Issue:** [#81](https://github.com/dannys-code-corner/incan/issues/81)  
**RFC PR:** —  
**Related:** RFC 013 (Rust crate dependencies)  
**Written against:** v0.1  
**Shipped in:** —  

## Summary

The Incan compiler currently emits `.unwrap()` calls in generated Rust code for operations that can fail at runtime. This causes cryptic Rust panics when users encounter edge cases. This RFC proposes a phased approach to improve error handling in generated code.

## Problem

Generated Rust code contains several `.unwrap()` patterns that can panic:

```rust
// Dict access - panics if key doesn't exist
map.get(&key).unwrap().clone()

// String parsing - panics if string isn't a valid number
input.parse::<i64>().unwrap()

// List indexing - panics if index out of bounds
vec[index]  // or explicit .get(i).unwrap()

// List.index() - panics if value not found
__incan_list_find_index(&vec, &value).unwrap()
```

When these panic, users see Rust stack traces instead of helpful Incan error messages:

```bash
thread 'main' panicked at 'called `Option::unwrap()` on a `None` value'
```

## Goals

1. **Clear error messages**: Users should see Incan-level errors, not Rust panics
2. **Source locations**: Errors should reference Incan source lines when possible
3. **Graceful degradation**: Some operations should return `None`/default instead of panicking
4. **Future-proof**: Design should support Incan-level error handling via `Result`/`Option`, `match`, and `?`

## Non-Goals

- Python-style `try/except` blocks — Incan handles errors via `match` on `Result` and `Option`, not exception catching.
- Changing the Incan language surface for operations that currently have implicit semantics (e.g., `d[key]` still raises a `KeyError`-equivalent, not returns `None`).
- Full source-map infrastructure — source location in runtime messages is best-effort for this RFC.

## Proposed Solution

### Phase 1: Better Panic Messages (Immediate)

Replace bare `.unwrap()` with `.unwrap_or_else()` that provides context:

```rust
// Before
map.get(&key).unwrap().clone()

// After
map.get(&key)
    .unwrap_or_else(|| panic!("KeyError: '{}' not found in dict", key))
    .clone()
```

For string parsing:

```rust
// Before
input.parse::<i64>().unwrap()

// After
input.parse::<i64>()
    .unwrap_or_else(|_| panic!("ValueError: cannot convert '{}' to int", input))
```

### Phase 2: Runtime Error Type

Introduce an Incan runtime error type that captures context:

```rust
// In generated prelude
#[derive(Debug)]
pub enum IncanError {
    KeyError { key: String, context: &'static str },
    ValueError { value: String, expected: &'static str },
    IndexError { index: usize, len: usize },
    TypeError { found: &'static str, expected: &'static str },
}

impl std::fmt::Display for IncanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IncanError::KeyError { key, context } =>
                write!(f, "KeyError: '{}' not found{}", key, context),
            // ... etc
        }
    }
}
```

### Phase 3: Optional Error Recovery

For operations where returning `None`/default makes sense:

```incan
# Dict.get() returns Option
value = my_dict.get("key")  # Returns Option[T]

# With default
value = my_dict.get("key", default="fallback")
```

Generated as:

```rust
let value = my_dict.get("key").cloned();  // Option<T>
let value = my_dict.get("key").cloned().unwrap_or("fallback".to_string());
```

### Phase 4: Incan-Level Error Handling (no `try/except`)

Incan should **not** adopt Python-style `try/except` blocks. Instead, error handling stays explicit and type-driven:

- Fallible operations return `Result[T, IncanError]` (or `Option[T]` where absence is expected)
- Callers handle errors via `match` (or propagate with `?`)

Example (handle a parse failure by falling back to a default):

```incan
# Incan uses a match expression to handle errors, instead of a try-except block (like Python)
match int(user_input):
    Ok(v) => value = v
    Err(_) => value = 0
```

Generated as:

```rust
let value = match user_input.parse::<i64>() {
    Ok(v) => v,
    Err(_) => 0,
};
```

> Note: this is where Incan is deliberately more rust-like than Python.

## Specific Improvements

### Dict Access

- **`d[key]`**
    - **Current**:

```rust
.get(...).expect("KeyError: key not found in dict").clone()
```

    - **Proposed**:

```rust
.get(...).unwrap_or_else(|| panic!("KeyError: key not found in dict")).clone()
```

    - **Note**: string-literal keys include the key in the message today.

- **`d.get(key)`**
    - **Proposed**:

```rust
.get(&k).cloned() // -> Option<V>
```

- **`d.get(key, default)`**
    - **Proposed**:

```rust
.get(&k).cloned().unwrap_or(default)
```

### String Parsing

- **`int(s)`**
    - **Current**: depends on input form
        - string literal:

```rust
.parse::<i64>().expect("ValueError: ...")
```

        - known `String` variable:

```rust
.parse::<i64>().unwrap_or_else(|_| panic!("ValueError: ...", s))
```

        - non-string:

```rust
(x as i64)
```

    - **Proposed**: parse strings with a `ValueError` message; cast non-strings for performance.

- **`float(s)`**
    - **Current**: depends on input form
        - string literal:

```rust
.parse::<f64>().expect("ValueError: ...")
```

        - known `String` variable:

```rust
.parse::<f64>().unwrap_or_else(|_| panic!("ValueError: ...", s))
```

        - non-string:

```rust
(x as f64)
```

    - **Proposed**: parse strings with a `ValueError` message; cast non-strings for performance.

### List Operations

- **`list[i]`**
    - **Current** (panics with Rust bounds message today):

```rust
vec[(i as usize)].clone()
```

    - **Proposed**:

```rust
vec.get(i as usize)
    .cloned()
    .unwrap_or_else(|| panic!("IndexError: index {} out of range for list of length {}", i, vec.len()))
```

- **`list.index(v)`**
    - **Current**:

```rust
__incan_list_find_index(&vec, &v).expect("ValueError: value not found in list")
```

    - **Proposed**:

```rust
__incan_list_find_index(&vec, &v).unwrap_or_else(|| panic!("ValueError: value not found in list"))
```

## Alternatives considered

### Always return `Result` for fallible operations

Make every fallible operation return `Result<T, IncanError>`. Rejected because:

- Requires pervasive `?` or `.unwrap()` in generated code
- Doesn't match Python/Incan semantics where these operations "just work"
- Would require pervasive `Result` handling (via `match`/`?`) to be usable

### Silent defaults

Return default values instead of panicking (e.g., `0` for missing int parse). Rejected because:

- Hides bugs
- Doesn't match Python semantics (Python raises exceptions)
- Makes debugging harder

## Drawbacks

- Phase 1 adds verbosity to generated Rust for every affected operation. This is noise in the generated output but has no runtime cost.
- Phase 2 introduces a shared prelude type (`IncanError`) that must be kept backward compatible as the language evolves.
- Phase 4 requires language-level changes (return-type inference for built-ins) that interact with the typechecker and lowering pipeline.

## Layers affected

- **Lowering / IR emission** — must replace bare unwrap patterns with named-message equivalents for all operations listed in "Specific Improvements"; must lower `dict.get(key)` and `dict.get(key, default)` to `Option`-returning and `unwrap_or`-based patterns respectively.
- **Generated prelude** — must introduce the `IncanError` enum (Phase 2) so all generated error messages share a common format.
- **Typechecker** — must track the return type of `dict.get(key)` as `Option[V]` (Phase 3); must eventually support `Result`-returning built-in variants (Phase 4).
- **Stdlib** — `dict.get`, `list.get`, and any `Result`-returning variants of `int()`/`float()` must be declared in `std` and wired to the appropriate Rust implementations.

## Unresolved questions

1. **Source location in runtime messages.** Should generated error messages include the Incan file and line number? If so, what mechanism threads that information into the generated Rust without significant overhead?

2. **`int()` / `float()` Result variants.** Should Phase 4 introduce `int.try_parse(s) -> Result[int, ValueError]` as a separate function, or should `int(s)` itself change return type when used in a `match`-position context?

3. **`IncanError` stability.** Once introduced in Phase 2, the `IncanError` enum is part of the implicit generated API. What versioning guarantees apply to it across compiler versions?

<!-- Rename the "Unresolved questions" section above to "Design Decisions" once all open questions have been resolved and the RFC moves to Planned status. -->
