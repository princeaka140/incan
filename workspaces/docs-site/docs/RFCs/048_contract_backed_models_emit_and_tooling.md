# RFC 048: Contract-backed models, Incan emit, and interrogation tooling

- **Status:** Draft
- **Created:** 2026-03-30
- **Author(s):** Danny Meijer (@dannymeijer)
- **Related:**
    - RFC 021 (model field metadata and aliases)
    - RFC 005 (Rust interop)
    - RFC 015 (project lifecycle and CLI tooling)
- **Issue:** #205
- **RFC PR:** —
- **Written against:** v0.2
- **Shipped in:** —

## Summary

This RFC specifies how Incan treats **canonical, machine-readable descriptions of row-shaped types** as first-class inputs alongside hand-written `model` declarations: the compiler **materializes** equivalent **nominal** types, guarantees **the same interrogation and reflection story** as handwritten models for the covered subset, and requires a **deterministic path** to **emit formatted Incan source** from that same description (“decompile” for human legibility). It also requires **editor and tooling hooks** so authors can **generate and review** that source in place—for example when shaping relational pipelines or data products—without falling back to YAML or opaque blobs as the only readable artifact.

## Core model

1. **Canonical model description** — A **versioned**, **machine-readable** bundle (exact encoding is specified with the feature; e.g. a stable schema record or interchange format) that names a row type and lists **fields** with **Incan types**, nullability, and optional **field-level metadata** aligned with RFC 021 where applicable.
2. **Materialization** — At compile time, the implementation **registers** a nominal type derived from that bundle so that uses of the type behave like a handwritten `model` of the same shape for typing, lowering, and reflection within the guarantees of this RFC.
3. **Emit (round-trip to source)** — Given the **same** canonical bundle (or a value that round-trips to it), the implementation **must** be able to produce **valid, formatted Incan** declaring a `model` (or the documented equivalent surface) that **re-parses** and **typechecks** to the same logical shape for the covered subset.
4. **Tooling** — The **same emit logic** used for standalone output **must** be available to **LSP** (or equivalent editor integration) via **documented commands** so users can insert or preview emitted source without a separate ad hoc formatter.

## Motivation

Hand-written `model` types are the most **readable** contract Incan offers, but many systems already carry row shape in **serialized** or **generated** form (schemas, plan outputs, registry artifacts). Today, making that shape a **real Incan type** often means **duplicate maintenance** or **external codegen** that drifts from the canonical bundle.

Authors and reviewers also need a **human-readable** view of the contract **inside the language**, not only in YAML or binary interchange, so diffs, code review, and governance workflows stay **idiomatic Incan**.

Finally, when a user is iterating on a **pipeline-shaped** or **dataset-shaped** surface, they should be able to **materialize the output row type as Incan** with **minimal friction**—including from the editor—to validate shape, attach tests, or align with policy, without treating the editor as a second-class consumer.

## Goals

- Define **normative guarantees** for **contract-backed** nominal types: **typing**, **lowering**, and **interrogation** (reflection, field metadata access where present) **must** match equivalent handwritten models for the **field subset** described by the contract.
- Require **deterministic emit**: same canonical input and emitter version **must** yield the **same** formatted Incan output (within the rules this RFC fixes for naming and field order).
- Require **tooling parity**: **LSP** (or documented editor protocol) **must** expose actions to **emit** or **preview** Incan model source derived from a **supported** in-editor selection or symbol context (exact triggers are specified here at the level of **capabilities**, not a single UI string).
- Align **field-level metadata** in the canonical bundle with **RFC 021** semantics where both apply, so governance and aliases do not fork between “source” and “contract” paths.

## Non-Goals

- Specifying the **full** type inference algorithm for arbitrary relational pipelines in host libraries; **companion specifications** may define how a host **produces** a canonical bundle for a given pipeline. This RFC defines what Incan **does** once a bundle is available and how tooling **surfaces** emit.
- **Perfect** round-trip of **comments**, **import organization**, or **author-only** formatting that is **not** represented in the canonical bundle.
- **Runtime-only** row types with **no** compile-time registration: this RFC targets **compiled** nominal types.
- Replacing handwritten `model` as the **primary** authoring style; contract-backed materialization is an **additional** path.

## Guide-level explanation

Authors and platform integrators treat a **canonical row description** as the **source of truth** for identity and interchange. The Incan toolchain **materializes** that as a real type in the program so generic APIs, tests, and Rust interop see **ordinary** models.

When someone needs to **read** or **review** the shape, they run **emit** (CLI or editor command): the tool prints or inserts **formatted Incan**—the same language they already use—instead of a parallel YAML dialect.

In the editor, after defining or selecting a **supported** pipeline or data-product surface, a **single command** can **generate the output model** as Incan text for **accuracy review**, **tests**, or **governance** checks, as long as the host or analyzer can supply a canonical bundle for that surface.

## Reference-level explanation (precise rules)

### Canonical model description

- A canonical description **must** include: a **logical type name** (or a documented rule for deriving a stable display name), a **format or schema version**, and an **ordered** list of fields.
- Each field **must** carry: a **field name**, an **Incan type** (or a mapping from a documented interchange type to Incan types), and **nullability** consistent with Incan’s model rules.
- Field entries **may** carry **metadata** keys and values compatible with RFC 021; if present, materialized types **must** expose the same metadata through the same reflection APIs as handwritten models.

### Materialization

- For every supported canonical bundle in scope of a compilation, the implementation **must** introduce a **nominal** type that:
  - participates in **name resolution** and **generic instantiation** like a declared `model` of the same field layout;
  - **lowers** to Rust with the **same structural guarantees** as an equivalent handwritten model for those fields;
  - supports **the same interrogation APIs** (e.g. field lists, schema-oriented accessors) as documented for handwritten models, for the covered subset.
- If a bundle is **ill-typed** or **incompatible** with the containing program, the implementation **must** emit **diagnostics** at compile time; it **must not** silently drop fields.

### Emit (decompile to Incan)

- Emit **must** produce **syntactically valid** Incan that declares a `model` (or the documented equivalent) whose **field set** and **types** correspond to the bundle.
- Emit **must** use the **project formatter** conventions so output matches **`make fmt`** (or documented formatter behavior) for the same Incan version.
- **Field order** in emitted source **must** follow the **canonical order** in the bundle unless this RFC’s unresolved questions adopt a different stable rule.
- Emit **need not** preserve **comments** or **non-contract** attributes; documentation **should** list what is **lossy**.

### Determinism

- For a fixed **canonical bundle**, **emitter version**, and **formatter version**, repeated emit **must** yield **identical** output (stable naming, stable spacing, stable field order per the chosen rule).

### Tooling (LSP)

- Implementations **must** provide at least one **editor-accessible** command that invokes the **same emit pipeline** as batch tooling, for **contexts** defined in this RFC once resolved (e.g. a symbol tied to a materialized type, or a host-supplied bundle for a selected construct).
- When emit is **not** available for the current context (unsupported construct, ambiguous shape), the implementation **must** surface a **clear diagnostic** rather than silent failure.
- **Security and reproducibility**: commands that accept external bytes **must** document trust boundaries; default behavior **should** prefer **in-memory** bundles already validated by the compiler or a trusted host.

### Rust interop

- Materialized types **must** follow the same **Rust export and import** rules as equivalent handwritten models (RFC 005), within the limits of the represented field set.

## Design details

### Relationship to handwritten `model`

- Handwritten `model` remains the **authoring** default. Contract-backed types are **additional** symbols that **must not** change the meaning of existing declarations.
- If a **name collision** occurs between a materialized type and a user-declared type, the language **must** specify a **hard error** or a **documented disambiguation** rule (see Unresolved questions).

### Identity and versioning

- Canonical bundles **should** carry a **logical identity** (e.g. hash or versioned id) for platform use. This RFC **does not** mandate a particular identity scheme but **requires** that **emitter** and **materialization** **do not** silently ignore **version** fields when they affect field layout.

### Companion specifications

- Host libraries or pipeline surfaces that **produce** canonical bundles **should** reference this RFC for **Incan-side** behavior and **may** define **producer** rules separately.

## Alternatives considered

1. **YAML (or JSON) as the only human-readable contract**
   - Familiar for infra, but **not** Incan: review and diffs **leave** the language ecosystem; duplicate mental models.

2. **External codegen only**
   - Works without language changes but **forks** formatting rules, **drifts** from compiler upgrades, and **weakens** editor integration.

3. **Reflection-only “anonymous” row types without nominal materialization**
   - Insufficient for **generic** APIs, **Rust interop**, and **stable** naming in large codebases.

## Drawbacks

- **Two paths** to the “same” shape (handwritten vs contract-backed) require **discipline** and **clear** diagnostics to avoid drift.
- **Deterministic emit** can **surprise** authors who expect **pretty** custom ordering unless the rules are **explicit**.
- **Tooling surface area** grows (commands, context detection, error messages).

## Implementation architecture

*(Non-normative.)* A single **shared** “bundle → AST → formatter” pipeline feeding both **CLI** and **LSP** reduces divergence. **Materialization** likely shares **lowering** with declared models once field lists are **normalized** to the same internal representation.

## Layers affected

- **Parser / AST**: may need nodes or attributes for **materialized** types or for **authoring hooks** that reference external bundles, depending on unresolved syntax choices.
- **Typechecker / Symbol resolution**: registration of **contract-backed** nominal types; collision and visibility rules.
- **IR Lowering / Emission**: shared path with handwritten `model` for represented fields.
- **Formatter**: ensure emitted `model` text is **idempotent** under format passes.
- **LSP / Tooling**: commands for **emit/preview**; optional code lenses tied to host capabilities.
- **Stdlib / Runtime**: reflection and metadata surfaces **must** stay consistent with RFC 021 for represented fields.

## Unresolved questions

1. **Authoring surface**: Is contract-backed materialization introduced via **explicit syntax** in `.incn` files, **build configuration**, **attributes** on imports, or a **combination**? What is the **minimum** v1 surface?
2. **Naming**: When the bundle’s logical name **collides** with a user `model`, is the error **always** on the user, or can materialized types use a **mangled** internal name with a **stable** alias?
3. **Field order for emit**: Strict **canonical order** only, or **sorted** identifiers for human browsing at the cost of non-literal round-trip to producer order?
4. **LSP context**: Which **constructs** must v1 support (e.g. only types already materialized in the project vs **host-provided** bundles for selected pipeline literals)?
5. **Partial bundles**: May a bundle **omit** types for some fields (opaque columns), and if so, how does Incan represent them **nominally**?

<!-- Rename this section to "Design Decisions" once all questions have been resolved.
     An RFC cannot move from Draft to Planned until no unresolved questions remain. -->
