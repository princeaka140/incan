# RFC 033: `ctx` — Typed Configuration Context


- **Status:** Draft
- **Created:** 2026-03-06
- **Author(s):** Danny Meijer (@dannymeijer)
- **Related:** RFC 032 (Value Enums)
- **Target version:** TBD

## Summary

Introduce `ctx` as a core language keyword that declares a **typed, globally accessible, environment-aware configuration context**. A `ctx` declaration looks like a `model` but produces a set-once singleton whose fields are resolved from a layered priority chain: field defaults → match-block overrides → environment variables. This replaces the YAML/env-var/dataclass juggling common in Python applications with a single, compile-time validated construct.

## Motivation

Every non-trivial application needs configuration — database paths, batch sizes, API endpoints, feature flags. Today's patterns are painful:

- **Python dataclass + env vars**: Manual `os.environ.get("KEY", "default")` for every field. No compile-time validation. String-typed. Error-prone.
- **Pydantic BaseSettings**: Typed, env-var-aware, but runtime-only validation. No multi-environment resolution. No language integration.
- **YAML + merge**: `common.yaml` + `prod.yaml` merged at load time. Untyped, no IDE support, merge surprises.
- **Django settings**: Python module importable globally. Untyped, single-environment, no structured overrides.
- **Koheesio Context**: Dict-like, YAML-loaded, `.get("key")` returns `Any`. 490 lines of Python + 40 lines of env resolution.

The common pain points:

1. **No compile-time safety** — config field references are strings, typos are runtime errors
2. **No environment awareness** — dev vs prod logic scattered across conditionals and YAML overrides
3. **Parameter threading** — config objects passed through 5+ layers of function calls
4. **No single source of truth** — defaults in code, overrides in YAML, env vars in shell, CLI flags in scripts

`ctx` solves all four in one language primitive.

### Who benefits

- **Application developers** — typed config with IDE completion, no boilerplate
- **Data engineers** — environment-aware pipeline config without YAML merge hierarchies
- **DevOps / platform teams** — env vars override defaults predictably; no code changes for per-environment tuning

## Guide-level explanation (how users think about it)

### Declaring a context

A `ctx` declaration defines a typed singleton with defaults and environment-specific overrides:

```incan
enum Env:
    Dev
    Prod

ctx AppConfig(env_prefix="APP_"):
    """Application configuration — typed, validated, environment-aware."""
    database_url: str = "sqlite:///dev.db"
    batch_size: int = 100
    debug: bool = true
    output_dir: str = "output"

    match Env:
        case Dev:
            debug = true
            batch_size = 10
        case Prod:
            database_url = "postgres://prod-host/app"
            debug = false
            batch_size = 5000
```

The declaration line — `ctx AppConfig(env_prefix="APP_"):` — controls **how** the context resolves (mechanics). The body declares **what** it contains (content). They never mix.

### Reading context fields

Context fields are accessed by type name — no variable, no parameter passing:

```incan
def connect() -> Connection:
    return Connection(AppConfig.database_url)

def process_batch(items: list[Item]) -> list[Result]:
    # AppConfig.batch_size is just... available. No parameter needed.
    for chunk in items.chunks(AppConfig.batch_size):
        ...
```

`AppConfig.batch_size` is a global read. The compiler knows the type (`int`) at compile time. IDE completion works. Rename-refactoring works.

### How env_prefix works

`ctx AppConfig(env_prefix="APP_")` maps each field to an environment variable using `{env_prefix}{UPPER_SNAKE_CASE_FIELD_NAME}`:

|     Field      |      Env var       |         Lookup order          |
| -------------- | ------------------ | ----------------------------- |
| `database_url` | `APP_DATABASE_URL` | env var → match arm → default |
| `batch_size`   | `APP_BATCH_SIZE`   | env var → match arm → default |
| `debug`        | `APP_DEBUG`        | env var → match arm → default |

The active `Env` variant is determined by the `{env_prefix}ENV` environment variable — e.g., `APP_ENV=Prod`.

A single shell command configures the entire application:

```bash
APP_ENV=Prod APP_BATCH_SIZE=10000 incan run
```

That command:

1. Resolves `match Env: case Prod:` — sets database_url, debug, batch_size from the match arm
2. Overrides `batch_size` from the `APP_BATCH_SIZE` env var (higher priority than the match arm)

### Resolution priority

Values resolve in this order (highest priority wins):

```
Field defaults (in code)         ← lowest
    ↓
Match block overrides            ← env-specific
    ↓
Environment variables            ← highest (runtime override)
```

This matches what every ops engineer already expects: "set it in the environment and it overrides everything."

### Per-field env var customization

All fields read from env vars by default (via `env_prefix` + `UPPER_SNAKE_CASE`). For fields that need a custom env var name, use field metadata:

```incan
ctx AppConfig(env_prefix="APP_"):
    database_url: str = "sqlite:///dev.db"           # reads from APP_DATABASE_URL
    auth_token [env="API_TOKEN"]: str                # reads from API_TOKEN (custom name)
    internal_state [env=false]: str = "computed"     # never read from env
```

This reuses the existing `[key=value]` field metadata syntax (same pattern as `[alias="type_"]` on models).

### Multiple context types

A program can have multiple `ctx` declarations for different concerns:

```incan
ctx AppConfig(env_prefix="APP_"):
    database_url: str = "sqlite:///dev.db"
    debug: bool = true

ctx InfraConfig(env_prefix="INFRA_"):
    cluster_size: int = 2
    region: str = "us-east-1"
```

Each is independently initialized and globally accessible: `AppConfig.debug`, `InfraConfig.region`.

### Usage in tests

Tests can reinitialize a context with specific values:

```incan
@test
def test_prod_config() -> Result[None, Error]:
    AppConfig.init(Env.Prod)
    assert_eq(AppConfig.debug, false)
    assert_eq(AppConfig.database_url, "postgres://prod-host/app")
    Ok(None)

@test
def test_custom_override() -> Result[None, Error]:
    AppConfig.init(Env.Dev, batch_size=42)
    assert_eq(AppConfig.batch_size, 42)     # override beats match arm
    Ok(None)
```

`.init()` resets the singleton in test context, even though it's one-shot in production.

## Reference-level explanation (precise rules)

### Syntax

```
ctx_decl     ::= "ctx" IDENT "(" ctx_params ")" ":" ctx_body
ctx_params   ::= ctx_param ("," ctx_param)*
ctx_param    ::= IDENT "=" expr
ctx_body     ::= NEWLINE INDENT (field_decl | match_block | docstring)+ DEDENT
match_block  ::= "match" IDENT ":" NEWLINE INDENT case_arm+ DEDENT
case_arm     ::= "case" IDENT ":" NEWLINE INDENT field_override+ DEDENT
field_override ::= IDENT "=" expr NEWLINE
```

**`ctx` is a core keyword** — always reserved, not a soft keyword. It has unique semantics (singleton, env var integration, match blocks) that don't fit `model` or `class`.

### Declaration rules

1. `ctx` name must be a valid identifier.
2. `env_prefix` parameter is required — it controls env var mapping and makes the mapping explicit.
3. The body contains **field declarations** (same syntax as `model` fields) and **match blocks**.
4. Match blocks reference a user-defined `enum` type. The enum must be in scope.
5. Match arms can only override fields declared in the same `ctx`. They cannot introduce new fields.
6. Fields without defaults and without coverage in all match arms are a compile error — the context must be fully resolvable.

### Type checking rules

1. **Field types** — same as `model` fields. Primitive types (`str`, `int`, `float`, `bool`), `Option[T]`, `list[T]`, and model types are all valid.
2. **Match arm overrides** — the assigned value must be type-compatible with the field's declared type. `batch_size: int = 100` can only be overridden with an `int`.
3. **Global access** — `AppConfig.field_name` is typed using the field's declared type. The typechecker resolves it as a static field access on a known singleton, not a regular instance access.
4. **Immutability** — context fields are read-only after initialization. Assigning to `AppConfig.field = x` outside of a match arm or `.init()` is a compile error.

### Env var type coercion

Environment variables are strings. The runtime coerces them to the declared field type:

| Field type  | Env var value  |                  Coercion                   |
| ----------- | -------------- | ------------------------------------------- |
| `str`       | `"hello"`      | No conversion needed                        |
| `int`       | `"42"`         | `str::parse::<i64>()`                       |
| `float`     | `"3.14"`       | `str::parse::<f64>()`                       |
| `bool`      | `"true"`/`"1"` | Case-insensitive truthy check               |
| `Option[T]` | `""`/absent    | `None` if empty/absent, parse `T` otherwise |

If coercion fails, the program exits at startup with a clear error message naming the env var, the expected type, and the actual value.

### Axis resolution

For each `match EnumType:` block in a `ctx`, the runtime reads `{env_prefix}{ENUM_TYPE_NAME}` from the environment — e.g., `APP_ENV` for `match Env:`. The value must match an enum variant name (case-insensitive). If the env var is absent, the match block is skipped entirely and field defaults are used.

### Lifecycle

1. **Before init** — accessing any `ctx` field panics with `"AppConfig not initialized"`. If the compiler can prove a field is accessed before `main()`, it emits a compile error.
2. **Init** — at program startup (or explicitly via `.init()` in tests): resolve axis env vars → evaluate matching match arms → apply env var field overrides → lock the singleton.
3. **After init** — `AppConfig.field` is a zero-cost read. Immutable. Thread-safe.
4. **In tests** — `.init()` can be called again (resets the singleton) for per-test configuration.

### Rust lowering

The compiler generates:

```rust
use std::sync::OnceLock;

struct AppConfigData {
    database_url: String,
    batch_size: i64,
    debug: bool,
    output_dir: String,
}

static APP_CONFIG: OnceLock<AppConfigData> = OnceLock::new();

fn init_app_config() {
    let env_variant: Option<Env> = std::env::var("APP_ENV")
        .ok()
        .and_then(|s| s.parse().ok());

    // Start with defaults
    let mut database_url = String::from("sqlite:///dev.db");
    let mut batch_size: i64 = 100;
    let mut debug: bool = true;
    let mut output_dir = String::from("output");

    // Apply match arm overrides
    if let Some(env) = env_variant {
        match env {
            Env::Dev => {
                debug = true;
                batch_size = 10;
            }
            Env::Prod => {
                database_url = String::from("postgres://prod-host/app");
                debug = false;
                batch_size = 5000;
            }
        }
    }

    // Apply env var overrides (highest priority)
    if let Ok(val) = std::env::var("APP_DATABASE_URL") {
        database_url = val;
    }
    if let Ok(val) = std::env::var("APP_BATCH_SIZE") {
        batch_size = val.parse().expect("APP_BATCH_SIZE must be a valid integer");
    }
    // ... etc for each field

    APP_CONFIG.set(AppConfigData { database_url, batch_size, debug, output_dir })
        .expect("AppConfig already initialized");
}
```

`AppConfig.batch_size` in Incan becomes `APP_CONFIG.get().unwrap().batch_size` in Rust — a pointer dereference after the `OnceLock` is set.

**Note:** The `.expect()` and `.unwrap()` calls here are in *generated user-program Rust code*, not in the compiler itself. The compiler's own `#[deny(clippy::unwrap_used)]` policy does not apply to code it emits into user projects. The panics are intentional: init-time failures (bad env var format) use `.expect()` for fail-fast startup errors; field accesses use `.unwrap()` because a missing `OnceLock` value is a programming error (accessing a context before it is initialized). Users who enable `unwrap_used` or `expect_used` in their own project's clippy configuration will see diagnostics on these generated calls — see unresolved question 6 for the trade-off discussion.

## Design details

### `ctx` vs `model` — how they relate

`ctx` and `model` share field declaration syntax but serve fundamentally different purposes:

|                         |             `model`              |                     `ctx`                     |
| ----------------------- | -------------------------------- | --------------------------------------------- |
| **What it is**          | A data shape — many instances    | The application's runtime world — a singleton |
| **How many**            | Many instances, created freely   | Exactly one, initialized once                 |
| **How you access it**   | Via a variable: `user.name`      | Via the type name: `AppConfig.database_url`   |
| **How it's created**    | `User(name="Alice", age=30)`     | Automatic at startup (or `.init()` in tests)  |
| **Mutability**          | Per-instance (mutable via `mut`) | Immutable after init — read-only globally     |
| **Match blocks**        | No                               | Yes — environment-specific overrides          |
| **Env var integration** | No                               | Yes — fields auto-read from env vars          |

**Mental model: `model` describes data. `ctx` describes the world the data lives in.**

### `ctx` contains models, not the other way around

A `ctx` can contain `model` instances as field values:

```incan
model ClusterConfig:
    node_type: str
    min_workers: int
    max_workers: int

ctx InfraConfig(env_prefix="INFRA_"):
    cluster: ClusterConfig = ClusterConfig(
        node_type="m5.xlarge",
        min_workers=2,
        max_workers=4,
    )

    match Env:
        case Prod:
            cluster = ClusterConfig(
                node_type="m5.4xlarge",
                min_workers=8,
                max_workers=16,
            )
```

Nested access works naturally: `InfraConfig.cluster.node_type`. The flow is one-directional — **ctx contains models, models don't contain ctx**. A `Transaction` model should never reference `AppConfig`.

### Interaction with existing features

**Traits:** `ctx` types do not implement traits. They are singletons, not polymorphic data types.

**async/await:** `ctx` fields are synchronously available (no `await` needed) — they're resolved at startup. Async functions can read ctx fields freely.

**Imports/modules:** `ctx` declarations follow normal module visibility. A `pub ctx` in a library can be imported by consumers. But: ctx initialization is the consumer's responsibility, not the library's.

**Error handling:** Env var parsing failures at init time produce a clear startup error (not a `Result` — fail fast). This is intentional: misconfigured environments should crash immediately, not silently propagate.

**Field metadata:** `ctx` fields support the same `[key=value]` metadata syntax as model fields. Currently defined metadata keys for ctx fields: `env` (custom env var name or `false` to disable).

### Compatibility / migration

This is a new feature — no migration needed. `ctx` becomes a reserved keyword, which is a breaking change for anyone using `ctx` as an identifier. Since Incan is pre-1.0, this is acceptable.

## Alternatives considered

### Decorator on model (`@ctx`)

```incan
@ctx(env_prefix="APP_")
model AppConfig:
    database_url: str = "sqlite:///dev.db"
    ...
```

**Rejected.** The `match Env:` blocks don't fit naturally inside a `model` body. The singleton semantics, env var integration, and match resolution are different enough from `model` to warrant a distinct keyword. A decorator would hide the fundamental semantic difference.

### Runtime-only config (like Pydantic BaseSettings)

Use a regular model with runtime env var reading:

```incan
model AppConfig:
    database_url: str
    batch_size: int

config = AppConfig.from_env(prefix="APP_")
```

**Rejected.** Loses compile-time validation of field references. Requires passing `config` around as a parameter. No match-block environment overrides. This is just a typed version of what Python already has.

### Soft keyword via library

Ship `ctx` as a library-provided soft keyword instead of a core keyword.

**Rejected.** `ctx` is universally useful — not specific to data pipelines or any domain library. Every application needs configuration. Making it a library feature would mean the most basic use case (read a setting from an env var) requires a dependency. The env var integration and singleton semantics are also deeply wired into the compiler's code generation.

## Drawbacks

- **New keyword** — `ctx` becomes reserved, breaking any code using it as an identifier. Acceptable pre-1.0.
- **Global mutable state** — technically a singleton, which some consider an anti-pattern. Mitigated by being immutable after init and resettable in tests.
- **Hidden dependency** — functions that read `AppConfig.field` have an implicit dependency on the context being initialized. Not visible in the function signature. This is the intended trade-off: explicitness vs. boilerplate reduction.

## Layers affected

- **Lexer** (`crates/incan_syntax/`) — `ctx` must be added as a core (always-reserved) keyword.
- **Parser** (`crates/incan_syntax/`) — parse `ctx Name(params):` declarations with field declarations and `match EnumType:` blocks; reuse field parsing from model declarations where possible.
- **AST** (`crates/incan_syntax/`) — a new `CtxDecl` node is required. The following data model captures the normative shape of what the AST must represent:

  ```rust
  pub struct CtxDecl {
      pub visibility: Visibility,
      pub name: Ident,
      pub params: Vec<(Ident, Spanned<Expr>)>,    // e.g., env_prefix="APP_"
      pub fields: Vec<Spanned<FieldDecl>>,
      pub match_blocks: Vec<Spanned<CtxMatchBlock>>,
  }

  pub struct CtxMatchBlock {
      pub axis: Spanned<Ident>,                    // the enum type name
      pub arms: Vec<Spanned<CtxMatchArm>>,
  }

  pub struct CtxMatchArm {
      pub variant: Spanned<Ident>,                 // e.g., Dev, Prod
      pub overrides: Vec<(Ident, Spanned<Expr>)>,  // field = value assignments
  }
  ```
- **Typechecker** (`src/frontend/typechecker/`) — validate field types; validate match arm override types are compatible with the declared field type; validate enum axes are in scope; enforce that every field has a default or exhaustive match arm coverage; register the ctx type in the symbol table so `AppConfig.field` resolves as a typed static field access.
- **IR Lowering** (`src/backend/ir/lower/`) — lower `CtxDecl` to IR: struct definition, `OnceLock` static, and init function body.
- **IR Emission** (`src/backend/ir/emit/`) — emit the Rust struct, `OnceLock` static, init function with match arms and env var reading logic, and field accesses as static reads through the lock.
- **CLI / main bootstrap** (`src/cli/`) — auto-insert `init_<ctx_name>()` calls at the start of the generated `main()` when the resolution is set to auto-init.
- **Formatter** (`src/format/`) — format `ctx` declarations (field alignment, match block indentation).
- **LSP** — completions and hover for `CtxName.field` access, showing field types and mapped env var names.
- **Test runner** — support `.init()` calls in test functions to reset the singleton per-test.

## Unresolved questions

1. **Should fields without defaults require exhaustive match coverage?** If `auth_token: str` has no default and `match Env` only covers `Prod`, what happens in `Dev`? Options: compile error (require all arms) or require a default value. Leaning toward: **fields without defaults must be covered by all match arms or have a matching env var annotation** — fail at init time if the env var is also absent.

2. **Should `env_prefix` be optional?** Could default to `{UPPER_SNAKE_CASE_CTX_NAME}_`. Leaning toward: **required** — explicit is better than implicit for something that maps to external env vars.

3. **How should nested model fields map to env vars?** `cluster: ClusterConfig` — should `INFRA_CLUSTER__NODE_TYPE` work (using a delimiter)? Or should nested models only be overridable as a whole? The research doc suggests `env_nested_delimiter` (Pydantic-style). Leaning toward: **defer nested env var mapping** — MVP covers flat fields only. Nested models are overridden in match arms.

4. **Auto-init or explicit init?** Should the compiler automatically insert `init_app_config()` at the start of `main()`? Or must the user call `AppConfig.init()`? Auto-init is more ergonomic; explicit init gives control over ordering when there are multiple ctx types. Leaning toward: **auto-init in `main()`**, with `.init()` available for tests and explicit control.

5. **Multiple match axes scope.** The research doc describes multi-axis matching (`match RunMode:`, compound `match (Env, RunMode):`). This RFC covers single-axis matching only. Should multi-axis be a follow-up RFC, or included here? Leaning toward: **follow-up RFC** — single-axis covers the common case and keeps the initial implementation focused.

6. **Generated panic compatibility with user clippy settings.** The generated init function uses `.expect()` for env var parse failures and `.unwrap()` for field access through the `OnceLock`. Both are intentional (fail-fast startup errors and programming-error detection respectively), but users who enable `clippy::unwrap_used` or `clippy::expect_used` in their own project will see diagnostics on these generated calls. Should the generated init function return `Result<(), CtxInitError>` and propagate errors to `main` via `?` instead of panicking? This would remove the `expect` calls but require users to handle init errors explicitly. Leaning toward: **keep panics for v1** — the fail-fast semantics are the point, and adding a `Result` return type complicates auto-init in `main`. A `#[allow(clippy::unwrap_used)]` annotation on the generated init function may be the right mitigation.

## Future extensions (not in this RFC)

The following capabilities are out of scope for this RFC and may be addressed in follow-up RFCs:

- **Multi-axis matching** — `match RunMode:`, `match LoadCadence:`, compound `match (Env, RunMode):`
- **CLI override** — `incan run --ctx batch_size=50` or `--ctx Env=Prod`
- **Dotenv support** — `env_file=".env"` parameter on the declaration line
- **Secrets** — `Secret[str]` type that enforces runtime-only resolution, never embedded in binary
- **Config validation** — `@validate` decorator on ctx for custom validation rules
- **`env_nested_delimiter`** — `"__"` delimiter for nested model field env var mapping
- **`case_sensitive`** — case-sensitive env var lookup (default: false)
