# RFC 072: `std.logging` — Pythonic structured logging on a tracing-backed runtime

- **Status:** Draft
- **Created:** 2026-04-23
- **Author(s):** Danny Meijer (@dannymeijer)
- **Related:**
    - RFC 022 (namespaced stdlib modules and compiler handoff)
    - RFC 023 (compilable stdlib and Rust module binding)
    - RFC 041 (first-class Rust interop authoring)
    - RFC 063 (`std.process` process spawning and command execution)
    - RFC 066 (`std.http` HTTP client surface)
- **Issue:** https://github.com/dannys-code-corner/incan/issues/392
- **RFC PR:** —
- **Written against:** v0.2
- **Shipped in:** —

## Summary

This RFC proposes `std.logging` as Incan's standard library surface for ordinary application and library logging. The user-facing model is intentionally Pythonic: acquire a logger with `get_logger(...)`, configure the root logger once near program startup with `basic_config(...)`, and emit messages with methods such as `log.info("message")`. Under that simple surface, log records remain structured events with optional fields and context, so the runtime can map them onto Rust's `tracing` ecosystem rather than committing Incan to a text-only or `println`-shaped logging story.

## Core model

Read this RFC as one foundation plus four mechanisms:

1. **Foundation:** Logging is a stdlib capability, not a Rust interop recipe and not a language keyword.
2. **Mechanism A:** Users acquire hierarchical `Logger` values through `get_logger(...)`, and the common case should feel as simple as `log.info("message")`.
3. **Mechanism B:** Log calls are structured events with a message, a level, a logger name, and optional fields or bound context, not just raw text writes.
4. **Mechanism C:** Applications configure logging once near startup; libraries emit logs but should not own process-wide logger configuration.
5. **Mechanism D:** The runtime should preserve compatibility with Rust's real logging world by mapping onto `tracing` and interoperating with the `log` facade where practical.

## Motivation

Logging is one of the first places where a language either feels credible for real programs or immediately falls back to ad hoc glue. CLIs, services, automation scripts, libraries, test helpers, and future query-language surfaces all need a shared story for diagnostics. Today, the escape hatch is Rust interop. That is serviceable, but it is not a stable language surface. It leaks Rust crate names, Rust backend assumptions, and Rust macro-oriented ergonomics into code that should stay ordinary Incan.

That leak is already visible in the project's own documentation. The Rust interop docs currently show `import rust::tracing as log` followed by `log.info("Starting server...")`. That proves the underlying capability exists, but it is not a coherent standard-library contract. Users should not need to know whether Rust prefers `log`, `env_logger`, `tracing`, or some other crate just to say "emit an info log here".

Python gets an important part of this right: the everyday developer experience is simple, hierarchical, and library-friendly. A module acquires a logger, the application configures the root logger, and records flow upward through the hierarchy. That model is learnable, composable, and already familiar to a large part of Incan's target audience.

However, copying Python's API shape is not enough by itself. Rust's ecosystem has a real distinction between the lightweight `log` facade and the richer `tracing` model. That distinction matters, especially for async code. A simplistic text-only logger would feel familiar for a week and limiting for years. Incan should therefore adopt the Python-shaped user model while keeping structured fields and tracing-oriented runtime semantics as the architectural center.

This also matters for operational hygiene. Logging surfaces tend to grow accidental bad habits: duplicate global configuration, eager expensive debug formatting, secrets dumped into output, and inconsistent correlation identifiers. Prior art from Scala and production Python code shows that convenience and discipline need to coexist. `std.logging` should therefore optimize for simple call sites without turning the runtime contract into ambient magic.

## Goals

- Provide a first-class `std.logging` module for application and library diagnostics.
- Make the common path simple and recognizably Pythonic: `get_logger(...)`, `basic_config(...)`, and `log.info("message")`.
- Standardize hierarchical logger names and root-oriented configuration.
- Make structured fields and bound context first-class so logs are useful for machines as well as humans.
- Keep library emission separate from application-wide logger configuration.
- Give the runtime a backend model that fits Rust reality by aligning with `tracing` and interoperating with `log` where practical.
- Prefer conservative defaults around log level, root output, and sensitive field handling.

## Non-Goals

- Exposing Rust logging crates directly as the stdlib contract.
- Reproducing Python's full handler / formatter / filter object graph in the first version.
- Defining distributed tracing, OpenTelemetry export, or metrics collection in this RFC.
- Making logging a keyword, decorator-only surface, or compiler special form.
- Standardizing every feature of `tracing-subscriber`, `log4rs`, `env_logger`, or other backend crates.
- Mirroring Python's exception-oriented logging helpers one-to-one where the language model does not justify them.

## Guide-level explanation

### Basic use

Programs configure logging once near startup and then acquire named loggers where they need them:

```incan
from std.logging import Level, basic_config, get_logger

basic_config(level=Level.INFO)

log = get_logger()

def main() -> None:
    log.info("Starting session")
```

The intended user experience is ordinary and unsurprising:

- use `basic_config(...)` in the executable or top-level script
- use `get_logger(...)` in modules and libraries
- call `debug`, `info`, `warning`, `error`, `critical`, or `trace` on the resulting logger

### Libraries should emit, not configure

Libraries should not take over process-wide logging policy. They should simply acquire a logger and emit records:

```incan
from std.logging import get_logger

log = get_logger()

pub def preview(dataset_name: str) -> None:
    log.debug("Preparing preview", fields={"dataset": dataset_name})
```

The application decides where those records go and which levels are visible.

### Hierarchical logger names

Named loggers should be hierarchical and dot-separated:

```incan
from std.logging import get_logger

session_log = get_logger("inql.session")
csv_log = get_logger("inql.session.csv")
```

This keeps filtering and output understandable. Logs from `inql.session.csv` are visibly related to `inql.session`, and root configuration can treat them as part of one tree.

For the common case, `get_logger()` with no explicit name should use the current module path when that information is available, so users are not forced to spell the module name manually in every file.

### Structured fields

The logging API should support additional fields without forcing users into manual string interpolation:

```incan
from std.logging import get_logger

log = get_logger()

def load_rows(dataset: str, row_count: int) -> None:
    log.info("Loaded dataset rows", fields={"dataset": dataset, "rows": row_count})
```

Human-readable output may render these fields textually, while JSON or tracing-oriented output may preserve them as structured values.

### Bound context

For repeated context such as request IDs or pipeline run IDs, users should be able to derive a contextual logger instead of repeating the same fields on every call:

```incan
from std.logging import get_logger

log = get_logger()

def handle_request(request_id: str) -> None:
    request_log = log.with_fields({"request_id": request_id})
    request_log.info("request started")
    request_log.info("request finished", fields={"status": 200})
```

This is the intended answer for correlation-friendly logging. The logger stays simple, but the emitted records remain structured.

### Checking whether a level is enabled

For expensive debug-only work, users should be able to check the effective level explicitly:

```incan
from std.logging import Level, get_logger

log = get_logger()

def debug_plan() -> None:
    if log.is_enabled(Level.DEBUG):
        log.debug(render_debug_summary())
```

The goal is not to force this pattern for ordinary logs. The goal is to provide an explicit escape hatch when the message construction itself is materially expensive.

## Reference-level explanation

### Required module surface

`std.logging` must provide, at minimum:

- `Level`
- `Logger`
- `get_logger(name: str | None = None) -> Logger`
- `basic_config(...)`

The root `Logger` type must provide, at minimum:

- `trace(message, fields=...)`
- `debug(message, fields=...)`
- `info(message, fields=...)`
- `warning(message, fields=...)`
- `error(message, fields=...)`
- `critical(message, fields=...)`
- `is_enabled(level: Level) -> bool`
- `with_fields(fields=...) -> Logger`

Implementations may add more helpers, but the public contract must remain centered on the simple logger model above.

### Levels

The standard level set must include:

- `TRACE`
- `DEBUG`
- `INFO`
- `WARNING`
- `ERROR`
- `CRITICAL`

User-facing APIs must expose `warning`, not only `warn`.

Implementations may provide `warn` as a compatibility alias, but it should be documented as a legacy or compatibility spelling rather than as the preferred surface.

### Logger acquisition and identity

`get_logger(name)` must return a logger associated with the given name.

If `name` is omitted:

- implementations should use the current module path when that information is available
- implementations may fall back to the root logger when the current module path is unavailable

Multiple calls to `get_logger()` for the same effective logger name must return the same logical logger identity for the purposes of configuration, filtering, and output metadata. The implementation is not required to preserve object identity if a different representation achieves the same observable behavior, but it must not behave as though each call created an unrelated logger namespace.

Logger names must be hierarchical and dot-separated. A logger named `a.b.c` must be treated as a descendant of `a.b` and `a`.

### Configuration model

`basic_config(...)` must configure the process-wide root logging behavior.

The first version of the contract must support, at minimum:

- configuring the minimum visible level
- configuring console output
- choosing a human-readable text format and a structured format such as JSON

The standard runtime should behave conservatively if the user does not call `basic_config(...)`:

- it should install a default root logger at level `WARNING`
- it should send human-readable output to standard error
- it should avoid silently promoting low-priority logs into visible output

Libraries should not call `basic_config(...)` except in test fixtures or executable-style examples.

If the runtime supports repeated configuration, that behavior must be explicit. A Python-like `force=True` model is acceptable. Silently reconfiguring global logging state on repeated ordinary calls is not.

### Log record content

Each emitted log record must preserve, at minimum:

- timestamp or an equivalent time source suitable for output formatting
- level
- logger name
- message text
- structured fields provided at the call site
- structured fields bound through `with_fields(...)`

Text output may render these into a line-oriented format. Structured output must preserve fields as fields rather than flattening everything into one preformatted string.

### Structured field semantics

The `fields=` argument must accept key-value data that can be carried across the logging boundary without forcing the user to manually interpolate the values into the message string.

Implementations must preserve scalar field values faithfully for structured sinks. They should preserve richer structured values where the configured sink format supports them.

When the same field key is present both in a logger produced by `with_fields(...)` and in a per-call `fields=` argument, the per-call field value should win.

### Effective-level checks

`is_enabled(level)` must reflect the logger's effective level under root and hierarchical configuration, not merely a local per-instance default.

The purpose of `is_enabled(level)` is to allow callers to guard expensive debug-only or trace-only computation. It must not itself emit log output or mutate logger state.

### Sensible sensitive-data behavior

The logging contract should treat obviously sensitive fields conservatively. Implementations should support redaction for keys such as `authorization`, `token`, `secret`, `password`, and similar credentials or bearer values.

The first version does not need a complete secret-type system, but it must not encourage APIs that casually dump sensitive structured data into debug-facing output by default.

### Library and application split

This RFC standardizes a strong separation of concerns:

- libraries should emit logs through `Logger`
- applications should configure root logging policy

Library code must not require a particular logging backend crate name in user-facing docs or examples. Users should not need to choose between `log` and `tracing` in ordinary Incan code.

## Design details

### Why the surface is Python-shaped

Python's standard logging model gets one crucial thing right for a general-purpose language: most code only needs a hierarchical logger and one root configuration point. That model is easy to teach, easy to compose across libraries, and familiar to the users Incan already targets. The key ergonomic choice in this RFC is therefore not novelty; it is choosing the boring path where boring is correct.

That is why `get_logger(...)`, `basic_config(...)`, dot-separated logger names, and root-oriented configuration are the center of the proposal. They make it easy for a user to read code and immediately understand where diagnostics come from.

### Why the runtime should be tracing-backed

Rust's `log` crate is intentionally a lightweight facade, and that separation between API and backend is valuable. But `tracing` exists for a reason. Its model is explicitly structured and async-aware, and it explains why plain text logs become hard to interpret once concurrent work is interleaved. Incan should not ignore that lesson.

Therefore this RFC deliberately keeps the public surface event-first and simple while recommending that the implementation preserve structured fields all the way down and map naturally onto `tracing` events. That gives Incan room to add richer scope or span-oriented diagnostics later without breaking the basic logger contract.

### Why this RFC does not start with a full span API

There is a temptation to expose `tracing` concepts directly because they are powerful. That would be the wrong first move. Most users need logging before they need tracing, and the proposal fails if the first thing an ordinary script author sees is a span taxonomy or a Rust crate adapter story.

The right order is the opposite:

- first, a strong event logger with names, levels, fields, and root configuration
- later, optional scope or span abstractions that layer on top of the same structured substrate

This keeps the entry path Pythonic without closing the door on tracing-oriented growth.

### Why root configuration stays narrow in v1

Python's logging module exposes handlers, filters, adapters, and formatters as a broad object graph. Rust's ecosystem exposes many backend crates and many subscriber layers. Both are powerful, and both are more than Incan needs to standardize on day one.

This RFC therefore keeps the committed configuration surface narrow: level, console output, and a choice between human-readable and structured output. That is enough to make the language credible without prematurely standardizing a large graph of handler types that would mostly mirror backend internals.

### Why `warning` is the preferred spelling

The Rust ecosystem commonly uses `warn`, but Python's standard library documents `warn` as obsolete and prefers `warning`. Since the explicit design target here is a Pythonic user-facing surface, the preferred method name should follow Python rather than Rust. A compatibility alias is acceptable, but it should not define the public style of the stdlib.

### Why context binding matters

Koheesio's logger factory and logger-ID filter illustrate a practical production concern: repeated context such as run IDs or correlation IDs should not require manual duplication across every log call. The `with_fields(...)` design is the minimal structured answer to that problem. It keeps the call sites readable while still producing records that backends can filter, serialize, or correlate.

This also interacts well with a tracing-backed implementation. Bound fields can map naturally onto structured event fields or future scope-oriented features without changing ordinary user code.

## Alternatives considered

- **Expose `rust::tracing` or `rust::log` directly**
  - Rejected because it leaks backend crate choices into ordinary Incan code and makes the stdlib story contingent on Rust crate literacy.
- **Copy Python logging wholesale**
  - Rejected because the full handler / formatter / filter object graph is too large a contract for the first version.
- **Make logging basically `println` with levels**
  - Rejected because it throws away structured fields, correlation context, and future tracing compatibility.
- **Start with spans instead of loggers**
  - Rejected because it optimizes for power users at the expense of the ordinary `log.info("message")` path the language needs first.
- **Standardize only the facade and leave startup behavior undefined**
  - Rejected because users need predictable default behavior, not a silent no-op logger surprise.

## Drawbacks

- This RFC intentionally leaves some advanced backend configuration unspecified, so power users will still want follow-up RFCs.
- A Python-shaped surface on top of a tracing-oriented runtime creates a real documentation burden; the contract has to stay simple without lying about structured behavior.
- Inferring the current module name for `get_logger()` without an explicit argument may require compiler or runtime support that is slightly more opinionated than a plain library function.
- Sensitive-field handling is easy to underspecify; if the redaction defaults are vague, users will fill the gaps with unsafe habits.

## Implementation architecture

*(Non-normative.)* A sensible implementation maps `Logger` method calls to `tracing` events, preserves the logger name as a `target`, and carries `fields=` data as native structured fields rather than eagerly flattening them into one string. The default executable bootstrap can install a conservative `tracing_subscriber` formatter at `WARNING` level so ordinary programs get visible warnings and errors without any manual setup.

Where Rust dependencies still emit through the `log` facade, the runtime should bridge that ecosystem into the same output path so mixed Rust crates and Incan code do not produce two unrelated logging systems. The public contract should continue to present one `std.logging` surface even if the runtime needs adapters underneath.

## Layers affected

- **Stdlib / runtime (`incan_stdlib`)**: new `std.logging` module, root configuration helpers, logger values, and structured field handling.
- **Compiler / stdlib handoff**: if `get_logger()` without a name infers the current module path, the language/runtime boundary must preserve that metadata.
- **Emission**: generated Rust output must map the stdlib logging surface onto the chosen runtime backend without losing levels, names, or structured fields.
- **LSP / Tooling**: completions and hover docs for `std.logging`, especially around `Level`, `basic_config(...)`, and `Logger` methods.
- **Docs / examples**: standard examples should stop teaching backend crate names for ordinary logging and instead center `std.logging`.

## Unresolved questions

- Should `get_logger()` with no explicit name infer the current module path in the first version, or should explicit names be required until module metadata is standardized more formally?
- Should the first version include a lazy-message helper beyond `is_enabled(level)` so Incan can capture more of Scala Logging's "simple call site, guarded expensive work" ergonomics?
- Should `basic_config(...)` in v1 include file output and JSON output, or should the committed surface stay even smaller and standardize only console behavior first?
- Should a future span or scope API live under `std.logging` as an additive extension, or should tracing-oriented scopes be a distinct follow-up RFC once plain event logging is implemented and used?

<!-- Rename this section to "Design Decisions" once all questions have been resolved.
     An RFC cannot move from Draft to Planned until no unresolved questions remain. -->
