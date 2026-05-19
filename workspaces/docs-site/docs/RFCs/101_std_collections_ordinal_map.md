# RFC 101: `std.collections.OrdinalMap` — deterministic compact key-to-ordinal lookup

- **Status:** Draft
- **Created:** 2026-05-19
- **Author(s):** Danny Meijer (@dannymeijer)
- **Related:**
    - RFC 009 (sized numeric types)
    - RFC 022 (namespaced stdlib modules and compiler handoff)
    - RFC 023 (compilable stdlib and Rust module binding)
    - RFC 030 (`std.collections` extended collection types)
    - RFC 052 (module static storage)
    - RFC 065 (`std.hash` stable hashing primitives)
    - RFC 086 (schema descriptors and adapters)
- **Issue:** https://github.com/dannys-code-corner/incan/issues/595
- **RFC PR:** —
- **Written against:** v0.3
- **Shipped in:** —

## Summary

This RFC proposes `std.collections.OrdinalMap`, a pure-Incan immutable map from deterministically encodable keys to compact integer ordinals. The type gives schemas, catalogs, generated metadata, and immutable dataset dictionaries a reproducible way to turn names or scalar values into stable integer codes, while keeping safe lookup exact, deterministic, serializable, and separate from mutable `dict` or general `FrozenDict` semantics.

## Core model

1. **Ordinal maps are immutable key-to-integer maps:** an `OrdinalMap[K]` maps each unique key of type `K` to a non-negative integer ordinal.
2. **Keys are deterministic encodings, not arbitrary hashables:** supported keys must implement a stable key-encoding contract so equal keys produce identical canonical bytes across platforms.
3. **Construction is deterministic:** the same canonical input must produce the same ordinals, lookup structure, and serialized bytes for a given Incan version and format version.
4. **Safe lookup is exact:** default lookup APIs must not silently return an ordinal for a missing key.
5. **Storage is compact:** ordinal cells should use the smallest supported unsigned storage width that can represent the maximum ordinal.
6. **Unchecked lookup is explicit:** a proven-present fast path may exist, but it must be visibly unsafe in the API and must not be the default behavior.
7. **The stdlib source owns the feature:** the public implementation should be authored in Incan, using existing stdlib primitives for hashing, bytes, typed integers, and I/O rather than hiding the data structure behind a hand-written Rust runtime type.

## Motivation

Large systems frequently need to resolve stable names or scalar values into integer positions before doing real work. Column names become column ordinals, field names become field IDs, table names become catalog IDs, enum-like values become compact codes, and string or scalar dictionary values become physical segment codes. Once a query, transformation, or scan has resolved a key to an ordinal, the hot path can use integer slots instead of repeated string comparisons.

Builtin `dict` is the right tool for mutable, general-purpose lookup. It is not the right public abstraction for immutable, reproducible, serialized lookup tables that should be rebuilt byte-for-byte across machines. `FrozenDict` is the right general frozen dictionary shape, but it is intentionally broad and should not be overloaded with specialized minimal-perfect-hash or ordinal-specific behavior.

The motivating requirement is not merely speed. Determinism matters because generated metadata, persisted catalogs, and immutable dataset segments must be reproducible. If the same schema or dictionary domain produces different index bytes depending on process randomness, host platform, or insertion-order accidents, downstream artifacts become harder to diff, cache, test, verify, and distribute.

Purpose-built data libraries also need a primitive below full physical indexing. A table with hundreds of columns and billions of rows should not resolve column names inside the row loop. The plan should resolve names to ordinals once, then execute against integer slots. For dictionary-encoded scalar columns, the segment dictionary should resolve a value such as `"paid"` or `Status.Paid` to an integer code once, then scan or compare packed integer codes.

This RFC is directly informed by Daniel Lemire's [`constmap`](https://github.com/lemire/constmap) work and by Thomas Mueller Graf and Daniel Lemire's [binary fuse](https://arxiv.org/abs/2201.01174) and [xor filter](https://arxiv.org/abs/1912.08258) papers. That work demonstrates the practical value of immutable compact maps built from a known key set, including fast lookup, compact storage, verified and unchecked lookup modes, and serialization. `OrdinalMap` adapts those lessons to Incan's stdlib goals: pure-Incan source ownership, deterministic construction, exact safe lookup, generic deterministic scalar keys, and ordinal/code-oriented semantics rather than a string-only `uint64` map.

## Goals

- Add `OrdinalMap[K]` to `std.collections` as an immutable, deterministic key-to-ordinal lookup type.
- Define a stable key-encoding contract for key types that can be used with `OrdinalMap`.
- Support construction from ordered keys where ordinals are input positions.
- Support construction from explicit key/ordinal pairs where ordinals are caller-provided.
- Require exact safe lookup semantics for default APIs.
- Define deterministic construction and deterministic serialization requirements.
- Select compact ordinal storage width from the maximum ordinal.
- Provide batch lookup APIs for resolving many keys without forcing callers into repeated boilerplate.
- Provide byte-size introspection and serialization/deserialization APIs suitable for persisted metadata.
- Keep the feature pure-Incan at the stdlib source level, with Rust used only as the normal generated-code and primitive-runtime substrate.

## Non-Goals

- This RFC does not replace builtin `dict`.
- This RFC does not replace `FrozenDict`.
- This RFC does not define a mutable ordered map.
- This RFC does not define a row-level database index, B-tree, bitmap index, inverted index, vector index, or range index.
- This RFC does not require `OrdinalMap` to support arbitrary user-defined objects in the first implementation slice.
- This RFC does not require floating-point keys in the first implementation slice.
- This RFC does not require compiler const-evaluation or pre-baked static emission in the first implementation slice, although the design should allow that optimization later.
- This RFC does not standardize probabilistic missing-key behavior as the default safe API.
- This RFC does not expose backend hash-table, minimal-perfect-hash, or fuse-filter internals as part of the public contract.

## Guide-level explanation

Use `OrdinalMap` when a stable set of keys needs stable integer positions.

```incan
from std.collections import OrdinalMap

columns = OrdinalMap.from_keys(["order_id", "customer_id", "status", "amount"])?

assert columns["status"] == 2
assert columns.get("missing").is_none()
```

The simplest construction form takes an ordered key list. The ordinal is the key's position in that list.

```incan
statuses = OrdinalMap.from_keys(["pending", "paid", "cancelled"])?

assert statuses["pending"] == 0
assert statuses["paid"] == 1
assert statuses["cancelled"] == 2
```

Explicit ordinals are useful when ordinals come from a file format, schema registry, generated catalog, or physical storage layout.

```incan
field_ids = OrdinalMap.from_pairs([
    ("order_id", 10),
    ("customer_id", 12),
    ("status", 18),
])?

assert field_ids["status"] == 18
```

Ordinal maps are not limited to strings. They can map any key with a stable canonical byte encoding.

```incan
customer_ids = OrdinalMap.from_keys([1001_u64, 1002_u64, 1003_u64])?
assert customer_ids[1002_u64] == 1
```

For dictionary-encoded data, an ordinal map can translate values into compact codes before execution enters a hot row loop.

```incan
status_codes = OrdinalMap.from_keys(["cancelled", "paid", "pending"])?
paid_code = status_codes.require("paid")?

# A scan can compare stored integer codes to `paid_code` instead of comparing strings for every row.
```

Batch lookup resolves many names or values at once.

```incan
wanted = columns.require_many(["status", "amount"])?
assert wanted == [2, 3]
```

Persisted metadata can serialize an ordinal map. Deserialization must validate the format, key encoding, construction metadata, and exact-lookup data before returning a usable map.

```incan
blob = columns.to_bytes()
round_trip = OrdinalMap[str].from_bytes(blob)?
assert round_trip["amount"] == 3
```

## Reference-level explanation

### Module and type

`std.collections` must expose `OrdinalMap[K]` as a public immutable collection type. `OrdinalMap[K]` maps keys of type `K` to non-negative integer ordinals.

`OrdinalMap[K]` must require `K` to satisfy the ordinal-key contract defined by this RFC or by the accepted final design. A key type that lacks a stable canonical encoding must not be accepted by `OrdinalMap`.

The type must be immutable after construction. APIs that appear to add, remove, or reassign entries must return a new map or be absent.

### Ordinal key contract

The stdlib must define an ordinal-key contract for key types that can be used with `OrdinalMap`. The contract must provide canonical bytes for a key value and must identify the key encoding used by serialized maps.

The exact trait name is unresolved. This RFC uses `OrdinalKey` as the descriptive name.

```incan
pub trait OrdinalKey:
    def ordinal_bytes(self) -> bytes: ...
    def ordinal_encoding(self) -> str: ...
```

The key encoding must be deterministic across supported platforms. It must not depend on locale, process-randomized hash seeds, pointer identity, memory layout, map iteration order, or implementation-defined object identity.

The initial supported key set should include `str`, `bytes`, `bool`, fixed-width signed and unsigned integers, and value enums whose values have stable scalar encodings. Additional scalar stdlib types such as UUIDs, dates, times, and datetimes may implement the contract when their canonical byte encodings are already stable.

`str` keys must encode their exact Unicode string value as UTF-8 bytes without implicit case folding, locale handling, or normalization. If callers need normalized keys, they must normalize before construction.

`bytes` keys must encode as their raw byte contents.

Fixed-width integer keys must encode using a specified byte order. This RFC requires little-endian two's-complement representation for signed fixed-width integers and little-endian representation for unsigned fixed-width integers unless the final design chooses a different single canonical byte order.

Floating-point keys are intentionally excluded from the required initial key set because NaN payloads, negative zero, and cross-language canonicalization rules need a separate decision.

### Construction from keys

`OrdinalMap.from_keys(keys: list[K]) -> Result[OrdinalMap[K], OrdinalMapError]` must construct a map where each key's ordinal is its zero-based position in the input list.

Construction from keys must reject duplicate keys. Duplicate detection must use the key's equality semantics and canonical ordinal bytes consistently. If two values compare equal, they must not appear as distinct keys. If two non-equal values produce the same canonical bytes, construction must fail with an encoding-collision error because the key contract has been violated.

For a fixed ordered input list, construction must be deterministic. The implementation may use hashing or retry-based placement internally, but all seeds, retries, placement choices, and serialized output must be derived deterministically from the canonical input and format version.

### Construction from pairs

`OrdinalMap.from_pairs(entries: list[Tuple[K, int]]) -> Result[OrdinalMap[K], OrdinalMapError]` must construct a map where each key maps to the explicit non-negative ordinal supplied by the caller.

Construction from pairs must reject duplicate keys. It must reject negative ordinals. It should reject duplicate ordinals by default because ordinal maps are primarily position/code maps, not many-to-one labels. If the final design permits duplicate ordinals, that behavior must be explicit in the constructor name or options.

Construction from pairs must be deterministic regardless of the input pair order when explicit ordinals are the same. The serialized representation should use a canonical order derived from key bytes or ordinal order so byte output is reproducible.

### Lookup APIs

`OrdinalMap[K].get(key: K) -> Option[int]` must return `Some(ordinal)` when `key` is present and `None` when it is absent.

`OrdinalMap[K].require(key: K) -> Result[int, OrdinalMapError]` must return the ordinal when present and a missing-key error when absent.

`OrdinalMap[K].__getitem__(key: K) -> int` should return the ordinal when present. The missing-key behavior of indexing must be consistent with Incan's collection indexing conventions. If ordinary indexing cannot return `Result`, `get` and `require` must remain the preferred APIs in examples that handle missing keys.

`OrdinalMap[K].__contains__(key: K) -> bool` must return whether `key` is present.

`OrdinalMap[K].get_many(keys: list[K]) -> list[Option[int]]` must perform batch lookup while preserving input order.

`OrdinalMap[K].require_many(keys: list[K]) -> Result[list[int], OrdinalMapError]` must perform batch lookup and return a missing-key error if any key is absent. The error should identify at least one missing key position and should identify all missing key positions when practical.

Safe lookup APIs must be exact. They must not return an ordinal for an absent key due only to hash or fingerprint collision. If an implementation uses fingerprints internally, it must verify enough key material to reject absent keys exactly before returning from safe APIs.

### Unchecked lookup

An unchecked lookup API may be provided for callers that can prove a key is present through external invariants or prior exact validation. The API must be explicitly named, such as `get_unchecked`, `require_unchecked`, or an accepted equivalent.

Unchecked lookup must not be used by `get`, `require`, `__getitem__`, `__contains__`, `get_many`, or `require_many`.

Unchecked lookup must document its missing-key behavior as undefined or implementation-specific. It must not silently become the default surface because the primary `OrdinalMap` contract is exact lookup.

### Deterministic storage and compact ordinals

An implementation should store ordinals using the smallest unsigned integer width that can represent the maximum ordinal in the map. The required width choices are `u8`, `u16`, `u32`, and `u64`.

A map whose maximum ordinal is at most `255` should use `u8` ordinal cells. A map whose maximum ordinal is at most `65535` should use `u16` ordinal cells. A map whose maximum ordinal is at most `4294967295` should use `u32` ordinal cells. Larger supported ordinals should use `u64` ordinal cells.

The public API returns ordinary Incan `int` ordinals unless a narrower return type is explicitly requested by a future RFC. Compact storage is an implementation and serialization property, not a narrowing of the public value.

### Serialization

`OrdinalMap[K].to_bytes() -> bytes` must return a deterministic byte representation for the map.

`OrdinalMap[K].from_bytes(data: bytes) -> Result[OrdinalMap[K], OrdinalMapError]` must parse and validate a serialized map before returning it.

Serialized ordinal maps must include at least a magic value, format version, key encoding identifier, key count, ordinal width, lookup algorithm identifier, exact-verification mode, construction metadata required for lookup, and the lookup/value payload. If the exact lookup mode requires stored key bytes or a key table, that data must be part of the serialized representation.

Serialization must use fixed byte order for numeric fields. This RFC requires little-endian numeric fields unless the final design chooses a different single canonical byte order.

Two maps constructed from equivalent canonical input under the same format version must serialize to identical bytes.

Deserialization must reject unknown required format versions, unsupported key encodings, malformed payloads, duplicate keys in stored exact-verification data, invalid ordinal widths, invalid counts, truncated payloads, and inconsistent payload lengths.

### Size introspection

`OrdinalMap[K].nbytes() -> int` should return the number of bytes retained by the ordinal-map payload, excluding ordinary object/header overhead that cannot be made portable across backends.

`OrdinalMap[K].serialized_size() -> int` should return the number of bytes `to_bytes()` would produce without requiring callers to materialize the serialized bytes when practical.

The documentation must distinguish payload size from total process heap usage.

### Error model

The stdlib must define `OrdinalMapError` or an equivalent stable error type. It must expose a stable category string and a human-readable message.

Error categories should include duplicate key, duplicate ordinal, negative ordinal, unsupported key type, key encoding collision, missing key, malformed serialized data, unsupported serialized version, unsupported key encoding, unsupported ordinal width, and lookup-construction failure.

### Relationship to `dict` and `FrozenDict`

`OrdinalMap` must not change builtin `dict` semantics.

`OrdinalMap` must not change `FrozenDict` semantics.

`OrdinalMap` should live in `std.collections` as a specialized collection for deterministic immutable ordinal lookup. It may share ordinary collection protocols such as `len`, membership, iteration, and indexing where those protocols fit the exact lookup contract.

## Design details

### Pure-Incan stdlib ownership

The user-facing type should be implemented in Incan stdlib source. That does not mean the generated program avoids Rust; Incan compiles to Rust. It means the data-structure algorithm, public methods, validation rules, and serialization contract are expressed as Incan code where practical.

The implementation may use existing stdlib primitives whose backing requires host support, such as stable hash algorithms, typed integer operations, byte buffers, and file I/O. It should not introduce a hand-written Rust-only `OrdinalMap` runtime type as the primary implementation because that would undermine the goal of demonstrating that Incan can express the data structure itself.

### Lookup algorithm freedom

This RFC does not standardize a specific minimal-perfect-hash or fuse-filter algorithm. The public contract is deterministic construction, exact safe lookup, compact ordinal storage, and deterministic serialization.

The implementation may use a binary-fuse-style structure, a minimal perfect hash family, a sorted table for small maps, or another deterministic layout as long as the public requirements hold. It may choose simpler representations for very small maps if that is faster or smaller after overhead.

### Prior art and acknowledgement

Daniel Lemire's [`constmap`](https://github.com/lemire/constmap) project is the immediate practical inspiration for this RFC. The project describes a compact immutable string-to-`uint64` map using the binary fuse filter construction, with lookup shaped around one hash computation, three array accesses, and XOR reconstruction. Its README also calls out the trade-off between unchecked lookup and verified missing-key detection, plus serialization for reuse. `OrdinalMap` should acknowledge that prior art while deliberately choosing a different public contract: deterministic generic ordinal keys, exact safe lookup, pure-Incan stdlib source, and persistent metadata suitability.

The algorithmic family comes from Thomas Mueller Graf and Daniel Lemire's ["Binary Fuse Filters: Fast and Smaller Than Xor Filters"](https://arxiv.org/abs/2201.01174) and the earlier ["Xor Filters: Faster and Smaller Than Bloom and Cuckoo Filters"](https://arxiv.org/abs/1912.08258). This RFC does not require Incan to standardize those algorithms, but they are important prior art for the space/time trade-offs motivating the feature.

### Deterministic retry policy

If the implementation uses retry-based construction, the retry sequence must be deterministic. A conforming strategy is to derive a base seed from the canonical key encodings, map format version, key encoding identifier, and algorithm identifier, then derive attempt seeds from that base seed and a monotonically increasing attempt number.

The implementation must define a deterministic failure mode if construction cannot find a valid layout within its supported attempt budget. Failure must return `OrdinalMapError`; it must not loop indefinitely.

### Exact verification

Safe lookup requires exact missing-key rejection. A compact implementation may use fingerprints as an early reject, but it must be able to verify the queried key exactly before returning an ordinal from safe APIs.

The exact verification data may be stored as canonical key bytes, a sorted side table, a collision bucket table, or another deterministic structure. The exact representation is not standardized by this RFC, but it must participate in `to_bytes()` and `from_bytes()` so persisted maps remain exact.

An explicitly probabilistic mode may be considered in the future for callers that accept false-positive risk. That mode must not be named or documented as the ordinary safe `OrdinalMap` contract.

### Key encoding and schema evolution

Serialized maps are tied to a key encoding identifier. If a type's canonical encoding changes in a backward-incompatible way, maps serialized with the old encoding must either continue to load through a compatibility path or fail with an unsupported key encoding error.

User-defined key encodings must be versioned if they can be persisted. The final design should decide whether user-defined `OrdinalKey` implementations can be used in serialized maps without an explicit encoding version.

### Dataset and metadata usage

For schema-like data, `OrdinalMap[str]` resolves field names or column names to integer positions. Query and transformation systems can resolve names once during planning, then execute against integer slots.

For immutable dictionary-encoded data, `OrdinalMap[K]` resolves a distinct scalar domain value to a compact code. A scan can compare stored integer codes rather than comparing full key values repeatedly.

For catalogs and generated metadata, `OrdinalMap` gives deterministic bytes that can be cached, diffed, shipped, and validated. This is more important than raw lookup speed alone because metadata reproducibility affects build systems, storage snapshots, and tests.

### API aliases

The accepted design may add aliases for common specializations. For example, `NameIndex` could be an alias for `OrdinalMap[str]` if the final naming discussion decides that schema/name lookup needs a clearer user-facing name.

Aliases must not obscure the generic contract. `OrdinalMap[K]` remains the normative type proposed by this RFC unless the RFC is renamed before acceptance.

## Alternatives considered

### String-only map

A string-only map is simpler and matches many schema and catalog use cases. It is too narrow for immutable scalar dictionaries, enum-coded columns, numeric identifiers, UUID-like values, and other deterministic scalar domains. The better boundary is a stable key-encoding contract.

### General frozen dictionary optimization

Optimizing `FrozenDict` would hide a specialized ordinal/code lookup contract inside a general mapping type. That would make missing-key semantics, compact ordinal storage, serialization, and unchecked lookup harder to explain. `FrozenDict` should remain the general frozen dictionary, while `OrdinalMap` owns the specialized deterministic ordinal use case.

### Builtin `dict` with conventions

Users can build a `dict[str, int]`, but that does not give deterministic serialized layout, compact ordinal storage, exact persisted lookup data, or a reproducible construction contract. `dict` remains appropriate for dynamic in-memory maps.

### Hand-written Rust runtime type

A hand-written Rust runtime type can be fast, but it sends the wrong message for a stdlib collection whose purpose is to show that Incan can own data-structure logic. Rust should remain the generated-code target and primitive substrate, not the only place where the actual collection exists.

### Fingerprint-only verified map

Fingerprint-only maps can be compact and fast, but they are not exact unless the fingerprint width is treated as part of an explicitly probabilistic contract. Persisted metadata and dataset dictionaries should not silently accept false positives. Fingerprints may be an optimization, but safe lookup must verify exact key membership.

### Full database index abstraction

A database index abstraction would need row identifiers, duplicate keys, range scans, null semantics, updates, segment visibility, and physical planning integration. `OrdinalMap` is smaller: it maps stable keys to integer ordinals. More complete physical indexes can build on ordinal maps where appropriate.

## Drawbacks

`OrdinalMap` adds a specialized collection to the stdlib, and specialized collections carry documentation and teaching cost. The type must justify itself through real uses in schemas, metadata, catalogs, and immutable dictionaries rather than through benchmark novelty.

Exact safe lookup costs more memory than fingerprint-only lookup. This is the right trade-off for persisted and correctness-sensitive data, but it means the most compact benchmark shape cannot be the default safe contract.

Generic key support requires a stable encoding story. If the key contract is too broad, serialized maps become fragile. If it is too narrow, the type becomes a string-only convenience. The initial key set must be chosen conservatively.

Construction can be more expensive than building a `dict`, especially for large maps. The type is intended for build-once, query-many workloads; it should not be promoted for one-off small maps.

Unchecked lookup is useful for proven-present hot paths, but it is easy to misuse. The API must make unsafety visible and examples should prefer exact lookup unless a caller has already validated presence.

## Implementation architecture

This section is non-normative.

The recommended shape is a pure-Incan implementation in `std.collections` backed by typed lists, stable hash helpers from `std.hash`, byte serialization helpers from `std.io` and `std.encoding`, and ordinary Incan model/enum definitions for the public error and representation choices.

The first implementation can choose a straightforward deterministic layout and add more compact layouts behind the same public contract after tests and benchmarks exist. A small-map representation may be better for tens of keys, while a minimal-perfect or fuse-style representation may be better for thousands or millions of keys.

Compiler support can later recognize literal or module-static `OrdinalMap.from_keys(...)` and precompute the serialized or in-memory representation at compile time. That optimization should preserve the same public Incan construction semantics rather than introducing a separate compiler-only behavior.

## Layers affected

- **Typechecker / Symbol resolution**: validates generic `OrdinalMap[K]` construction against the ordinal-key contract and reports unsupported key types.
- **IR Lowering**: may need to preserve enough static construction information for later compile-time baking, if that optimization is added.
- **Emission**: may eventually emit pre-baked deterministic ordinal-map payloads for module-static or const-compatible construction.
- **Stdlib / Runtime (`incan_stdlib`)**: adds the public pure-Incan `OrdinalMap` type, key-encoding contract, error type, constructors, lookup APIs, and serialization APIs.
- **LSP / Tooling**: should surface completions and hover documentation for `OrdinalMap`, supported key types, missing-key APIs, and unchecked lookup warnings.

## Unresolved questions

- Should the final public name be `OrdinalMap`, `NameIndex`, `StringIndex`, or another name that better communicates the type's intended use?
- What exact name and method shape should the stable key-encoding contract use?
- Which key types are required in the first implementation slice?
- Should `from_pairs` reject duplicate ordinals unconditionally, or should many-to-one explicit ordinals be supported through a separate constructor?
- What exact missing-key behavior should `__getitem__` use for `OrdinalMap`?
- Should unchecked lookup be included in the first implementation slice, or deferred until real users need the proven-present fast path?
- What exact serialized format version, magic value, and metadata fields should be standardized before Draft moves to Planned?
- What exact exact-verification representation should the first implementation use?
- Should a probabilistic fingerprint-only mode exist at all, or should it stay outside the stdlib?
- Should compiler const-baking be part of the acceptance criteria, or a later optimization after the pure-Incan stdlib implementation lands?

<!-- Rename this section to "Design Decisions" once all questions have been resolved.
     An RFC cannot move from Draft to Planned until no unresolved questions remain. -->
