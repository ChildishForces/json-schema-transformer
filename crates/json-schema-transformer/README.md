# json-schema-transformer

A Rust crate that converts JSON Schema (Draft 2020-12) into typed code for multiple languages. The converter parses any JSON Schema into a target-agnostic intermediate representation (SchemaIr), then emits code through pluggable emitter strategies.

## Features

Each emitter is an opt-in cargo feature: `zod`, `typescript`, `pydantic`, `swift`, `kotlin`, or `all-emitters` for everything. No emitters are enabled by default — the parser and IR are always available.

```toml
[dependencies]
json-schema-transformer = { version = "0.1", features = ["zod", "swift"] }
```

## Usage

```rust
use json_schema_transformer::{transform, ZodEmitter, PydanticEmitter, TypeScriptEmitter, SwiftEmitter, KotlinEmitter};
use serde_json::json;

let schema = json!({
    "type": "object",
    "properties": {
        "email": { "type": "string", "format": "email" },
        "age": { "type": "integer", "minimum": 0 }
    },
    "required": ["email"]
});

// Pick any emitter
let zod = transform(&schema, &ZodEmitter, "user-created", "users", 1).unwrap();
let ts = transform(&schema, &TypeScriptEmitter, "user-created", "users", 1).unwrap();
let py = transform(&schema, &PydanticEmitter, "user-created", "users", 1).unwrap();
let swift = transform(&schema, &SwiftEmitter, "user-created", "users", 1).unwrap();
let kt = transform(&schema, &KotlinEmitter, "user-created", "users", 1).unwrap();

// Or use convenience functions
use json_schema_transformer::json_schema_to_zod_module;
let zod = json_schema_to_zod_module(&schema, "user-created", "users", 1).unwrap();
```

## Architecture

### Pipeline

The converter uses a two-pass pipeline with a shared IR:

```
JSON Schema ──► input.rs (parse) ──► SchemaIr ──► emit/*.rs (emit) ──► Code
                                        │
                                        ├──► zod.rs         → .zod.ts
                                        ├──► typescript.rs   → .d.ts
                                        ├──► pydantic.rs     → .py
                                        ├──► swift.rs        → .swift
                                        └──► kotlin.rs       → .kt
```

### Parse (`input.rs`): JSON Schema to SchemaIr

- Pre-pass builds a `$id` registry (`HashMap<String, Value>`) for URI-based ref resolution
- Tracks `base_uri` scope for relative URI resolution within nested `$id` schemas
- Handles `$ref` cycle detection — recursive schemas are marked and emitted with lazy/forward references
- Resolves JSON Pointer escaping (`~0` → `~`, `~1` → `/`, percent decoding)
- `$ref` alongside sibling keywords: resolves the ref, builds a sibling IR, intersects both
- Topologically sorts definitions via Kahn's algorithm for correct emission order (dependencies before dependents)

### Intermediate Representation (`ir.rs`): SchemaIr

The `SchemaIr` enum has 25 variants covering the full JSON Schema vocabulary:

| Category           | Variants                                                                                                                     |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| **Primitives**     | `String(StringConstraints)`, `Number(NumericConstraints)`, `Integer(NumericConstraints)`, `Boolean`, `Null`                  |
| **Containers**     | `Object(ObjectSchema)`, `Array(ArraySchema)`, `Tuple(TupleSchema)`, `Record(RecordSchema)`                                   |
| **Enums/Literals** | `Enum(Vec<String>)`, `MixedEnum(Vec<LiteralValue>)`, `ComplexEnum(Vec<Value>)`, `Literal(LiteralValue)`, `ConstEqual(Value)` |
| **Composition**    | `Union(Vec<SchemaIr>)`, `ExclusiveUnion(Vec<SchemaIr>)`, `Intersection(Vec<SchemaIr>)`                                       |
| **Refinement**     | `Not { base, schema }`, `Conditional { base, if_, then_, else_ }`, `TypeGuarded(Vec<TypeGuard>)`                             |
| **References**     | `Ref(String)`, `Lazy(String)`                                                                                                |
| **Modifiers**      | `Nullable(Box<SchemaIr>)`, `Optional(Box<SchemaIr>)`, `Default(Box<SchemaIr>, Value)`, `Describe(Box<SchemaIr>, String)`     |
| **Special**        | `Any`, `Unknown`, `Never`                                                                                                    |

Key structural types:

```rust
struct ObjectSchema {
    fields: IndexMap<String, FieldEntry>,
    additional_properties: AdditionalProperties,
    pattern_properties: Vec<(String, SchemaIr)>,
    property_names: Option<Box<SchemaIr>>,
    min_properties: Option<u64>,
    max_properties: Option<u64>,
    dependent_required: Vec<(String, Vec<String>)>,
    dependent_schemas: Vec<(String, SchemaIr)>,
}

struct ArraySchema {
    items: Box<SchemaIr>,
    min_items: Option<u64>,
    max_items: Option<u64>,
    unique_items: bool,
    contains: Option<ContainsConstraint>,
}

struct TupleSchema {
    items: Vec<SchemaIr>,
    rest: Option<Box<SchemaIr>>,
    unique_items: bool,
    min_items: Option<u64>,
    max_items: Option<u64>,
    contains: Option<ContainsConstraint>,
}
```

### Emit (`emit/*.rs`): SchemaIr to Code

Each emitter implements the `Emitter` trait:

```rust
pub trait Emitter {
    fn extension(&self) -> &str;
    fn emit(&self, converted: &ConvertedSchema, name: &str, namespace: &str, version: u32) -> String;
}
```

Shared helpers in `emit/mod.rs`:

- `needs_quoting(key)` — whether a JS/TS property key needs quoting
- `needs_python_quoting(key)` — whether a Python identifier needs quoting
- `module_header(namespace, name, version)` — standard `// Generated by` comment block

All emitters produce versioned root type names: `{PascalName}V{version}Data` (e.g., `UserCreatedV1Data`).

## Emitters

### Zod (`emit/zod.rs`)

Generates Zod TypeScript validation schemas with full runtime constraint enforcement.

**Output example:**

```typescript
import { z } from 'zod';

export const UserCreatedV1DataSchema = z.object({
  email: z.string().email(),
  age: z.number().int().gte(0).optional(),
});
export type UserCreatedV1Data = z.infer<typeof UserCreatedV1DataSchema>;
```

**Spec compliance: 88.8%** (see [Spec Compliance](#spec-compliance))

| JSON Schema Feature                        | Zod Mapping                                                      |
| ------------------------------------------ | ---------------------------------------------------------------- |
| `type: "string"`                           | `z.string()`                                                     |
| `format: "email"`                          | `.email()`                                                       |
| `format: "uuid"`                           | `.uuid()`                                                        |
| `format: "uri"`                            | `.url()`                                                         |
| `format: "date-time"`                      | `.datetime()`                                                    |
| `format: "ipv4"`                           | `.ip({ version: "v4" })`                                         |
| `minLength` / `maxLength`                  | `.min(n)` / `.max(n)`                                            |
| `pattern`                                  | `.regex(/pattern/)`                                              |
| `minimum` / `maximum`                      | `.gte(n)` / `.lte(n)`                                            |
| `exclusiveMinimum` / `exclusiveMaximum`    | `.gt(n)` / `.lt(n)`                                              |
| `multipleOf`                               | `.multipleOf(n)`                                                 |
| `items`                                    | `z.array(itemSchema)`                                            |
| `prefixItems`                              | `z.array(z.any()).superRefine(...)` (positional checks)          |
| `uniqueItems`                              | `.superRefine(...)` with `_stableStringify` helper               |
| `contains` / `minContains` / `maxContains` | `.superRefine(...)` with match counting                          |
| `additionalProperties: false`              | `.strict()`                                                      |
| `additionalProperties: {schema}`           | `.catchall(schema)`                                              |
| `patternProperties`                        | `.passthrough().superRefine(...)` with regex checks              |
| `propertyNames`                            | `.superRefine(...)` on `Object.keys()`                           |
| `dependentRequired`                        | `.superRefine(...)` with `Object.hasOwn` checks                  |
| `dependentSchemas`                         | `.superRefine(...)` with nested schema validation                |
| `not`                                      | `.superRefine(...)` with negated `safeParse`                     |
| `if` / `then` / `else`                     | `.superRefine(...)` with conditional branching                   |
| `allOf` (objects)                          | Fields merged into single `z.object()`                           |
| `allOf` (non-objects)                      | `z.intersection(...)`                                            |
| `anyOf`                                    | `z.union([...])`                                                 |
| `oneOf`                                    | `z.union([...])` with exclusive superRefine                      |
| `$ref` (recursive)                         | `z.lazy(() => schema)` at both definition and use site           |
| `const` (primitive)                        | `z.literal(value)`                                               |
| `const` (object/array)                     | `.superRefine(...)` with `_deepEqual` helper                     |
| Typeless schemas                           | `z.any().superRefine(...)` with `typeof` guards per keyword type |

**Locally computed helpers** (only emitted when needed):

- `_stableStringify`: key-order-independent JSON serialization for `uniqueItems` object comparison
- `_deepEqual`: recursive structural equality for `const` with object/array values

### TypeScript (`emit/typescript.rs`)

Generates pure `.d.ts` type definitions with no runtime code.

**Output example:**

```typescript
export type UserCreatedV1Data = { age?: number; email: string };
```

| SchemaIr             | TypeScript                      |
| -------------------- | ------------------------------- |
| `String`             | `string`                        |
| `Number` / `Integer` | `number`                        |
| `Boolean`            | `boolean`                       |
| `Null`               | `null`                          |
| `Any` / `Unknown`    | `unknown`                       |
| `Never`              | `never`                         |
| `Array`              | `type[]`                        |
| `Tuple`              | `[type1, type2, ...rest[]]`     |
| `Object`             | `{ field: type; field?: type }` |
| `Record`             | `Record<string, type>`          |
| `Union`              | `type1 \| type2`                |
| `Enum`               | `"a" \| "b" \| "c"`             |
| `Literal`            | `"value"` / `42` / `true`       |
| `Nullable`           | `type \| null`                  |
| `Ref` / `Lazy`       | Referenced by name              |

**Limitations:** No runtime enforcement of any constraints. Numeric bounds, string patterns, array lengths, and all refinement keywords are not expressed in the output.

Objects with `$defs` produce named `export interface` declarations. Enums produce `export type X = "a" | "b" | "c"`.

### Pydantic (`emit/pydantic.rs`)

Generates Python `@pydantic_dataclass` classes with `Field()` constraints.

**Output example:**

```python
from __future__ import annotations
from pydantic import Field
from pydantic.dataclasses import dataclass as pydantic_dataclass
from typing import Annotated, Optional

@pydantic_dataclass
class UserCreatedV1Data:
    email: str
    age: Optional[int] = Field(ge=0, default=None)
```

**Spec compliance: 89.7%** (see [Spec Compliance](#spec-compliance))

| SchemaIr          | Python Type                 | Constraint Mapping                             |
| ----------------- | --------------------------- | ---------------------------------------------- |
| `String`          | `str`                       | `Field(min_length=N, max_length=N, pattern=P)` |
| `Number`          | `float`                     | `Field(ge=N, le=N, gt=N, lt=N, multiple_of=N)` |
| `Integer`         | `int`                       | `Field(ge=N, le=N, gt=N, lt=N, multiple_of=N)` |
| `Boolean`         | `bool`                      |                                                |
| `Null`            | `None`                      |                                                |
| `Any` / `Unknown` | `Any`                       |                                                |
| `Array`           | `list[ItemType]`            | `Field(min_length=N, max_length=N)`            |
| `Tuple`           | `tuple[Type1, Type2]`       |                                                |
| `Object` (inline) | `dict[str, Any]`            |                                                |
| `Object` (named)  | `@pydantic_dataclass class` |                                                |
| `Record`          | `dict[str, ValueType]`      |                                                |
| `Union`           | `Union[A, B]`               |                                                |
| `Enum` (strings)  | `Literal["a", "b"]`         |                                                |
| `MixedEnum`       | `Literal[1, "a", True]`     |                                                |
| `Nullable`        | `Optional[T]`               |                                                |
| `Ref` / `Lazy`    | `"ForwardRef"` (string)     |                                                |

**Field naming conventions:**

- Field names are converted to `snake_case` with `alias="originalName"` when they differ
- `populate_by_name: True` is added to config when aliases are present
- Required fields with constraints use `Annotated[Type, Field(...)]` to avoid default-ordering issues
- Optional fields use `Type = Field(..., default=None)`
- Required fields are always sorted before optional fields (Python dataclass rule)

**What Pydantic validates:**

- Type correctness (string vs int vs bool vs object vs array)
- Required vs optional fields
- `Literal` values (enum enforcement)
- Numeric bounds (`ge`, `le`, `gt`, `lt`, `multiple_of`)
- String constraints (`min_length`, `max_length`, `pattern`)
- Array length (`min_length`, `max_length`)
- `additionalProperties: false` via `extra="forbid"` config

Beyond native `Field()` constraints, the emitter generates `BeforeValidator` / `@model_validator` machinery for `uniqueItems`, `contains`, `patternProperties`, `propertyNames`, `dependentRequired`/`dependentSchemas`, `not`, `if`/`then`/`else`, property count bounds, schema-typed `additionalProperties`, and deep-equality `const`/enum checks. Remaining failures are tracked in the conformance results (see [Spec Compliance](#spec-compliance)).

### Swift (`emit/swift.rs`)

Generates `struct Name: Codable` with `CodingKeys` enum for field name mapping, plus validating `Codable` wrapper types for string formats.

**Output example:**

```swift
import Foundation

// ValidationError and EmailAddress are emitted only when needed
enum ValidationError: LocalizedError {
    case invalidEmail(String)
    // ...
}

struct EmailAddress: Codable, Hashable, CustomStringConvertible {
    let value: String
    init(_ value: String) throws {
        let pattern = try! Regex(#"[^@\s]+@[^@\s]+\.[^@\s]+"#)
        guard value.wholeMatch(of: pattern) != nil else {
            throw ValidationError.invalidEmail(value)
        }
        self.value = value
    }
    init(from decoder: Decoder) throws { /* validates on decode */ }
    func encode(to encoder: Encoder) throws { /* encodes as String */ }
}

struct UserCreatedV1Data: Codable {
    let age: Int?
    let email: EmailAddress  // Validated on decode!
}
```

| SchemaIr                       | Swift Type             | Validates                        |
| ------------------------------ | ---------------------- | -------------------------------- |
| `String`                       | `String`               |                                  |
| `String` (`format: email`)     | `EmailAddress`         | Regex on decode                  |
| `String` (`format: uuid`)      | `UUID`                 | Foundation validates             |
| `String` (`format: uri`)       | `URL`                  | Foundation validates             |
| `String` (`format: date-time`) | `Date`                 | Foundation validates             |
| `String` (`format: date`)      | `Date`                 | Foundation validates             |
| `String` (`format: time`)      | `TimeString`           | Regex on decode                  |
| `String` (`format: ipv4`)      | `IPv4Address`          | Octet validation on decode       |
| `String` (`format: ipv6`)      | `IPv6Address`          | Hex group validation on decode   |
| `String` (`format: ip`)        | `IPAddress`            | Tries IPv4 then IPv6             |
| `String` (`format: hostname`)  | `HostnameString`       | Label validation on decode       |
| `String` (`format: duration`)  | `DurationString`       | ISO 8601 regex on decode         |
| `String` (`format: base64`)    | `Base64String`         | `Data(base64Encoded:)` on decode |
| `Number`                       | `Double`               |                                  |
| `Integer`                      | `Int`                  |                                  |
| `Boolean`                      | `Bool`                 |                                  |
| `Null`                         | `Never?`               |                                  |
| `Any` / `Unknown`              | `AnyCodable`           |                                  |
| `Array`                        | `[ItemType]`           |                                  |
| `Tuple`                        | `[AnyCodable]`         |                                  |
| `Object` (inline)              | `[String: AnyCodable]` |                                  |
| `Record`                       | `[String: ValueType]`  |                                  |
| `Union` (nullable)             | `Type?`                |                                  |
| `Union` (other)                | `AnyCodable`           |                                  |
| `Enum`                         | `String`               |                                  |
| `Nullable`                     | `Type?`                |                                  |
| `Ref` / `Lazy`                 | Referenced by name     |                                  |

**Validating wrapper types** are self-contained `Codable` structs that validate during `init(from:)`. They:

- Conform to `Codable`, `Hashable`, and `CustomStringConvertible`
- Encode as plain `String` (transparent to JSON consumers)
- Throw a `ValidationError` with a descriptive message on invalid input
- Are only emitted when actually used (conditional import detection)

**Field naming:**

- Field names are converted to `camelCase`, preserving already-camelCase input (e.g., `tradeId` stays `tradeId`)
- A `CodingKeys` enum is generated when any field name differs from its JSON key
- Swift keywords are backtick-escaped

**What Swift validates at decode time:**

When any field has constraints, a custom `init(from decoder:)` is generated that validates after decoding:

- **Format types**: `UUID`, `URL`, `Date` (Foundation), `EmailAddress`, `IPv4Address`, `IPv6Address`, `IPAddress`, `HostnameString`, `TimeString`, `DurationString`, `Base64String` (custom Codable wrappers)
- **Numeric bounds**: `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`
- **String constraints**: `minLength`, `maxLength`, `pattern` (regex via `range(of:options:.regularExpression)`)
- **Array bounds**: `minItems`, `maxItems`
- **Enum values**: inline string enums validated against allowed values
- **Additional properties**: `additionalProperties: false` rejects unknown keys via dynamic key container

Complex compositions (`not`, `if`/`then`/`else`, `oneOf`, intersections, `patternProperties`, `dependent*`) are validated via generated wrapper types with custom `init(from:)` logic. Helper and `$defs` types are emitted file-private (or root-type-prefixed when they appear in public signatures) so many generated files can compile into a single module.

### Kotlin (`emit/kotlin.rs`)

Generates `@Serializable data class` with `@SerialName` annotations for kotlinx.serialization.

**Output example:**

```kotlin
package users.events

import kotlinx.serialization.Serializable

@Serializable
data class UserCreatedV1Data(
    val age: Int? = null,
    val email: String
)
```

| SchemaIr           | Kotlin Type                                   |
| ------------------ | --------------------------------------------- |
| `String`           | `String`                                      |
| `Number`           | `Double`                                      |
| `Integer`          | `Int`                                         |
| `Boolean`          | `Boolean`                                     |
| `Null`             | `Nothing?`                                    |
| `Any` / `Unknown`  | `JsonElement`                                 |
| `Array`            | `List<ItemType>`                              |
| `Tuple`            | `List<JsonElement>`                           |
| `Object` (inline)  | `JsonObject`                                  |
| `Record`           | `Map<String, ValueType>`                      |
| `Union` (nullable) | `Type?`                                       |
| `Union` (other)    | `JsonElement`                                 |
| `Enum` (strings)   | `String` (or `enum class` for top-level defs) |
| `Nullable`         | `Type?`                                       |
| `Ref` / `Lazy`     | Referenced by name                            |

**Package and imports:**

- Package derived from namespace: `{namespace}.events` (dashes/dots replaced with underscores)
- Imports are conditional — only `JsonElement`, `JsonObject`, `SerialName` are imported when actually used
- `@SerialName` is added when Kotlin field names (camelCase) differ from JSON keys
- Kotlin keywords are backtick-escaped

**Serialization types:** Uses `kotlinx.serialization.json.JsonElement` for untyped values and `JsonObject` for inline objects, ensuring all types are serializable without additional configuration.

**Validation:** Simple objects map to plain `@Serializable data class` (structure and required fields enforced by kotlinx). Schemas with constraints generate wrapper classes with custom `KSerializer` implementations performing the checks at decode time. Helper and `$defs` types are emitted file-private or root-type-prefixed so many generated files can compile into a single module.

## Spec Compliance

Measured against the official [JSON Schema Test Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite) (Draft 2020-12) — all 1,299 test cases, nothing skipped. Current baselines:

| Emitter      | Pass rate        |
| ------------ | ---------------- |
| **Zod**      | 88.8%            |
| **Pydantic** | 89.7%            |
| **Swift**    | 89.5%            |
| **Kotlin**   | 86.5%            |
| TypeScript   | N/A (no runtime) |

Biggest gaps (all emitters, converter-level): `unevaluatedProperties`/`unevaluatedItems`, `$dynamicRef`/`$dynamicAnchor`, `$anchor`, remote `$ref`s, plus `$id` scope edge cases and per-language `format` validator differences. Per-keyword failure breakdowns live in `conformance/results/<lang>.json` after a conformance run.

## Testing

Run from the repository root:

```bash
# Rust unit tests
cargo test -p json-schema-transformer --all-features

# Full pipeline: build, unit, fixture generation, all conformance suites (~20s)
bun scripts/test.ts
```

See the repository README for details on the native-language conformance harnesses.

## Dependencies

- `serde_json` — JSON parsing
- `indexmap` — Ordered maps for deterministic output
- `thiserror` — Error type derivation
