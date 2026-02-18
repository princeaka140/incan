# RFC 012: `JsonValue` Type, Enum Methods, and Enum Trait Adoption

- Status: Planned
- Author(s): Danny Meijer (@dannymeijer)
- Issue: #80
- RFC PR: —
- Created: 2025-11-15 (draft), 2026-02-17 (final)
- Related: 
    - [RFC 005] (Rust interop)
    - [RFC 023] (compilable stdlib & rust.module binding)
    - [RFC 025] (multi-instantiation trait dispatch)

## Summary

This RFC introduces three things:

1. **Enum methods** — extend Incan enums to support method declarations, bringing them to parity with models and
   classes.
2. **Enum trait adoption** — extend Incan enums to support `with Trait` syntax, enabling enums to adopt traits like
   `Index[K, V]` for subscript access.
3. **`std.json.JsonValue`** — an enum with methods and trait adoption for handling JSON with unknown or varying
   structure at runtime. `JsonValue` is the motivating use case for both enum features and the first stdlib type to
   use them.

## Motivation

### Enums can't have methods

Today, Incan enums can only declare variants. They cannot have methods:

```incan
# Current: enums are variant-only
enum Color:
    Red
    Green
    Blue

# No way to add methods — this doesn't parse:
#   def is_warm(self) -> bool: ...
```

This is a significant gap. Models and classes both support methods. Enums — which in Incan are algebraic data types
with tuple variants — are the only declaration type without them. This limits their usefulness for any type that needs
both variants and behavior.

### Enums can't adopt traits

Models and classes support `with Trait` to adopt traits (e.g., `model Matrix with Index[Tuple[int, int], float]`).
Enums cannot. This means enums can't implement protocols like `Index` for subscript access, which is essential for
`JsonValue["key"]` to work. Without enum trait adoption, `JsonValue` indexing would require special-case compiler
support rather than using the existing `Index` trait from `std.traits.indexing`.

### Dynamic JSON has no solution

Incan currently requires defining models for all JSON handling. This falls short for:

1. **Dynamic APIs** — APIs that return varying structures depending on context
2. **Exploration** — prototyping without defining full models upfront
3. **Partial parsing** — extracting specific fields from large JSON without modeling everything
4. **Mixed schemas** — JSON where some parts are typed and others are dynamic

A `JsonValue` type needs to be an enum (it represents one of: null, bool, int, float, string, array, object) **and** it
needs methods (`.as_str()`, `.is_null()`, `.parse()`, etc.). Without enum methods, `JsonValue` would require clunky
module-level functions instead of natural method-call syntax.

## Non-Goals

- **Typed JSON serialization** (`@derive(json)`, `.to_json()`, `.from_json()`). That is covered by [RFC 024] Phase 4,
  which provides the `std.serde.json` derivable module.
- **Enum pattern matching enhancements.** This RFC adds methods and trait adoption to enums; it does not change `match`
  semantics.

## Guide-level explanation (how users think about it)

### Enum methods (general feature)

Enums can now have methods, just like models and classes:

```incan
enum Direction:
    North
    South
    East
    West

    def is_horizontal(self) -> bool:
        match self:
            East => return true
            West => return true
            _ => return false

    def opposite(self) -> Direction:
        match self:
            North => return Direction.South
            South => return Direction.North
            East => return Direction.West
            West => return Direction.East
```

Methods on enums follow the same rules as methods on models/classes:

- `self` receiver for instance methods
- No receiver for associated functions
- Can be `@rust.extern` for Rust-backed implementations
- Support type parameters on the enum and on individual methods

### Enum trait adoption (general feature)

Enums can now adopt traits with `with`, just like models and classes:

```incan
from std.traits.indexing import Index

enum Lookup with Index[str, int]:
    Mapping(Dict[str, int])
    Empty

    def __getitem__(self, key: str) -> int:
        match self:
            Lookup.Mapping(d) => return d[key]
            Lookup.Empty => return 0
```

This brings enums to full parity with models and classes for trait adoption. The `with` clause appears after the enum
name (and optional type parameters), before the colon.

### `JsonValue` — dynamic JSON

```incan
from std.json import JsonValue

# Parse unknown JSON
data = JsonValue.parse(response_body)?

# Navigate dynamically
name = data["user"]["name"].as_str()
count = data["items"].as_int()

# Type inspection
if data["payload"].is_object():
    println("payload is an object")

# Serialize back to string
json_str = data.to_json()
```

### Mixing typed and dynamic

With [RFC 024]'s `@derive(json)`, a model can have `JsonValue` fields for partially-dynamic schemas:

```incan
from std.serde import json
from std.json import JsonValue

@derive(json)
model ApiResponse:
    status: int
    message: str
    data: JsonValue  # Dynamic payload — structure varies per endpoint

response = ApiResponse.from_json(body)?
user_name = response.data["user"]["name"].as_str()
```

## Reference-level explanation (precise rules)

### Enum methods

#### Enum declaration syntax

Enum declarations are extended to allow method declarations after the variant list:

```incan
enum TypeName[T]:
    Variant1
    Variant2(field_type)
    Variant3(type1, type2)

    def method_name(self) -> ReturnType:
        # method body

    def associated_function() -> ReturnType:
        # no self — associated function
```

Methods are separated from variants by a blank line (by convention, not enforced). The parser continues to parse
variants until it encounters `def` (or a decorator followed by `def`), then switches to parsing methods.

#### AST changes

`EnumDecl` gains `traits` and `methods` fields:

```rust
EnumDecl {
    visibility: Visibility,
    decorators: Vec<Spanned<Decorator>>,
    name: Ident,
    type_params: Vec<TypeParam>,
    traits: Vec<Spanned<Ident>>,            // NEW — adopted traits
    variants: Vec<Spanned<VariantDecl>>,
    methods: Vec<Spanned<MethodDecl>>,      // NEW — enum methods
}
```

`EnumInfo` in the symbol table gains `traits` and `methods` fields:

```rust
EnumInfo {
    type_params: Vec<String>,
    traits: Vec<String>,                    // NEW
    variants: Vec<String>,
    methods: HashMap<String, MethodInfo>,   // NEW
}
```

#### Typechecking

- Enum methods are collected in the first pass (`collect_enum`), same as model/class methods.
- Method bodies can reference `self` and use `match self` to dispatch on variants.
- The enum's type parameters are in scope within method bodies.

#### Lowering and emission

- Enum methods lower to `IrFunction` entries associated with the enum type, same as model/class methods.
- Emitted as `impl TypeName { fn method_name(&self, ...) { ... } }` in Rust.

### `std.json.JsonValue`

#### Type definition

> Note: this is a pseudo-code example showing the shape of the type. It is not a complete implementation.

```incan
# stdlib/json.incn
rust.module("incan_stdlib::json")

from std.traits.indexing import Index

enum JsonValue with Index[str, JsonValue], Index[int, JsonValue]:
    Null
    Bool(bool)
    Int(int)
    Float(float)
    String(str)
    Array(List[JsonValue])
    Object(Dict[str, JsonValue])

    # ---- Rust-backed primitives ----

    def parse(json_str: str) -> Result[JsonValue, str]: ...
    def to_json(self) -> str: ...

    # ---- Constructors ----

    def null() -> JsonValue: ...
    def from_bool(value: bool) -> JsonValue: ...
    def from_int(value: int) -> JsonValue: ...
    def from_float(value: float) -> JsonValue: ...
    def from_string(value: str) -> JsonValue: ...
    def from_array(items: List[JsonValue]) -> JsonValue: ...
    def from_object(entries: Dict[str, JsonValue]) -> JsonValue: ...

    # ---- Type inspection ----

    def is_null(self) -> bool: ...
    def is_bool(self) -> bool: ...
    def is_int(self) -> bool: ...
    def is_float(self) -> bool: ...
    def is_string(self) -> bool: ...
    def is_array(self) -> bool: ...
    def is_object(self) -> bool: ...

    # ---- Value extraction ----

    def as_bool(self) -> Option[bool]: ...
    def as_int(self) -> Option[int]: ...
    def as_float(self) -> Option[float]: ...
    def as_str(self) -> Option[str]: ...
    def as_array(self) -> Option[List[JsonValue]]: ...
    def as_object(self) -> Option[Dict[str, JsonValue]]: ...

    # ---- Indexing (from Index trait adoption, see RFC 025) ----

    def __getitem__(self, key: str) -> JsonValue: ...
    def __getitem__(self, key: int) -> JsonValue: ...

```

#### Rust backing

`JsonValue` is backed by `serde_json::Value` at the Rust level. The implementation will use `rust.module()` and/or
`rust::` interop to bridge the two — the specifics are left to the implementer. Users who need direct access to the
underlying Rust type can use `rust::` interop:

```incan
from rust::serde_json import Value
```

#### Indexing semantics

`JsonValue` supports subscript access with both `str` keys (object field access) and `int` keys (array element access):

```incan
# Object field access — returns JsonValue.Null if key is missing
value["key"] -> JsonValue

# Array index access — returns JsonValue.Null if out of bounds
value[0] -> JsonValue

# Chained access
value["users"][0]["name"].as_str()
```

Indexing returns `JsonValue` (not `Option[JsonValue]`), with missing keys / out-of-bounds indices returning
`JsonValue.Null`. This enables ergonomic chaining without intermediate unwrapping.

The intended design is trait-based: `JsonValue` adopts both `Index[str, JsonValue]` and `Index[int, JsonValue]`, with
two `__getitem__` implementations disambiguated by argument type. This requires multi-instantiation trait dispatch,
proposed in [RFC 025]. Until RFC 025 is implemented, indexing may use compiler-level support as an interim fallback
(the same way `List` and `Dict` indexing works today).

#### Numeric handling

JSON has a single number type. Incan has `int` and `float`. The mapping:

- JSON integers (no decimal point) → `JsonValue.Int(int)`
- JSON floats (with decimal point or exponent) → `JsonValue.Float(float)`
- `.as_int()` on a `Float` value returns `None` (no silent truncation)
- `.as_float()` on an `Int` value returns `Some(value as float)` (widening is safe)

The implementer might want to add a generic helper function to convert numerical strings to their correct type.
For example:

```incan
def parse_number(s: str) -> JsonValue:
    if s.contains("."):
        return JsonValue.Float(float.parse(s))
    else:
        return JsonValue.Int(int.parse(s))
```

## Design details

### Syntax

Two new syntactic elements for enums:

1. **`with Trait` clause** — optional, after the enum name and type parameters, before the colon. Parsed identically to
   the existing `with` clause on models and classes (this means that `@derive(...)`-decorator will also be supported).
2. **Method declarations inside enum bodies** — after all variants, the parser looks for `def` (or decorators followed
   by `def`) and parses method declarations using the same `method_decl` parser already used for models and classes.

```incan
enum Name[T] with Trait1, Trait2:
    Variant1
    Variant2(T)

    def method(self) -> T: ...
```

### Semantics

Enum methods behave identically to model/class methods:

- Instance methods receive `self` (immutable) or `mut self` (mutable)
- Associated functions have no receiver
- Methods can use `match self` to dispatch on variants
- Type parameters from the enum declaration are in scope

### Interaction with existing features

#### `match` expressions

Enum methods complement `match` — they don't replace it. Pattern matching remains the primary way to destructure enum
variants from outside the type. Methods provide behavior that the enum encapsulates internally.

#### `@rust.extern` on enum methods

`@rust.extern` works on enum methods the same way it works on model/class methods — delegates to the Rust backing
module declared by `rust.module()`. This is how `JsonValue`'s `parse()` and `to_json()` primitives are backed.

#### `JsonValue` as a model field type

`JsonValue` can be used as a field type in models. When the model derives JSON serialization ([RFC 024]), `JsonValue`
fields serialize as their JSON representation:

```incan
from std.serde import json
from std.json import JsonValue

@derive(json)
model Event:
    event_type: str
    payload: JsonValue  # Serializes as whatever JSON structure it holds
```

### Compatibility / migration

**Enum methods**: fully additive. Existing enums without methods continue to work unchanged. The parser only looks for
methods after all variants are parsed.

**`JsonValue`**: entirely new. No migration needed.

## Alternatives considered

### 1. `JsonValue` as a model with opaque Rust interior

Use a model (not an enum) backed by `serde_json::Value`, with all methods on the model. This works with current Incan
but loses the ability to pattern match on JSON variants:

```incan
# Can't do this without an enum:
match data:
    JsonValue.String(s) => println(f"got string: {s}")
    JsonValue.Int(n) => println(f"got int: {n}")
    _ => println("other")
```

Pattern matching is a natural fit for JSON type dispatch. An enum with methods is the right abstraction.

### 2. Module-level functions instead of methods

Keep enums method-free, put `JsonValue` functions at module level:

```incan
from std.json import JsonValue, parse, as_str, is_null

data = parse(body)?
name = as_str(data["user"]["name"])
```

Functional but clunky — no method chaining, no `.` syntax, poor discoverability via IDE completion.

### 3. `Dict[str, Any]` for dynamic JSON

Rust doesn't have `Any` like Python. Would require boxing and type erasure, losing the ability to distinguish JSON
types (null vs missing, int vs float, etc.).

## Drawbacks

- **Enum methods are a language change**: extends the parser, AST, typechecker, lowering, and emission. However, the
  mechanism is a straightforward extension of existing model/class method support — no new concepts are introduced.
- **`JsonValue` is dynamically typed**: introduces runtime type inspection into a language that emphasizes static
  typing. This is intentional and scoped — `JsonValue` is opt-in for specific use cases, not a general-purpose `Any`.
- **Rust dependency**: `JsonValue` maps to `serde_json::Value`, adding a hard dependency on `serde_json`. This crate
  is already a dependency for model serialization.

## Implementation plan

### Phase 1: Enum methods and trait adoption (language feature)

- [ ] Extend the AST and symbol table to support methods and `with` trait adoption on enums
- [ ] Update the parser to accept `with` clauses and method declarations in enum bodies
- [ ] Update the typechecker to collect and validate enum traits and methods
- [ ] Update lowering and emission to generate Rust `impl` blocks for enum methods and trait implementations
- [ ] Add codegen snapshot and integration tests for enums with methods and trait adoption

### Phase 2: `std.json.JsonValue` type

- [ ] Define the `JsonValue` enum in `stdlib/json.incn` with the surface described in this RFC
- [ ] Implement the Rust backing module wrapping `serde_json::Value`
- [ ] Implement all methods (constructors, type inspection, value extraction, `parse`, `to_json`)
- [ ] Add codegen snapshot tests

### Phase 3: Indexing support

- [ ] Implement `Index[str, JsonValue]` and `Index[int, JsonValue]` via `__getitem__` (depends on [RFC 025])
- [ ] Alternatively, add compiler-level subscript support as an interim fallback
- [ ] Verify chained access works: `value["users"][0]["name"].as_str()`
- [ ] Add codegen snapshot tests for indexing and chained access

### Phase 4: `JsonValue` as a model field type

- [ ] Ensure `JsonValue` fields in `@derive(json)` models ([RFC 024]) serialize/deserialize correctly
- [ ] Add codegen snapshot tests for hybrid models (typed + dynamic fields)

### Phase 5: Documentation

- [ ] User guide: enum methods and trait adoption
- [ ] User guide: dynamic JSON with `JsonValue`
- [ ] API reference for `std.json`

## Design decisions

1. **Enum methods and trait adoption are general-purpose**: although `JsonValue` motivates these features, they are not
   `JsonValue`-specific. Any enum benefits. The implementation follows the same pattern as model/class methods and
   trait adoption.

2. **Null fallback for indexing**: `value["missing_key"]` returns `JsonValue.Null` rather than `Option[JsonValue]`.
   This matches `serde_json::Value` behavior and enables ergonomic chaining. Users who need explicit missing-key
   detection use `.as_object()` and check the dict.

3. **Safe numeric widening**: `.as_float()` on an `Int` value returns `Some(value as float)` because widening is
   lossless. `.as_int()` on a `Float` returns `None` because truncation is lossy and should be explicit.

4. **Minimal `@rust.extern` surface**: only `parse()` and `to_json()` are `@rust.extern` — they call `serde_json`
   functions that cannot be expressed in Incan. All other methods (constructors, type inspection, value extraction) are
   pure Incan using enum variant constructors and pattern matching.

## Deferred questions

1. **`value.key` sugar**: should `value.key` be syntactic sugar for `value["key"]` on `JsonValue`? Convenient for
   exploration but adds typechecker complexity. Deferred to a future RFC.

2. **Pretty printing**: should `.to_json()` accept formatting options, or should pretty printing be a separate
   function?

3. **Streaming / incremental parsing**: large JSON documents may benefit from streaming parsers. Out of scope —
   `JsonValue.parse()` is eager (loads entire document into memory).

4. **Module-level convenience functions**: the implementation may add module-level aliases like `json.parse(s)` and
   `json.dumps(value)` mirroring Python's `json.loads()` / `json.dumps()` convention, as syntactic sugar over
   `JsonValue.parse()` and `value.to_json()`.

5. **Trait-based indexing**: `JsonValue` indexing depends on multi-instantiation trait dispatch ([RFC 025]) to adopt
   both `Index[str, JsonValue]` and `Index[int, JsonValue]`. Until RFC 025 is implemented, the compiler may use
   built-in indexing support as a fallback.

## References

- [RFC 024] — Extensible Derive Protocol
- [RFC 025] — Multi-Instantiation Trait Dispatch
- Rust `serde_json` crate — `serde_json::Value`
- Python `json` module — `json.loads()`, `json.dumps()`

--8<-- "_snippets/rfcs_refs.md"
