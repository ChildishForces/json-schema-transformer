# json-schema-transformer

Transform JSON Schema (Draft 2020-12) into native types and validation schemas for multiple languages. A single Rust core parses any JSON Schema into a target-agnostic intermediate representation (`SchemaIr`), then pluggable emitters generate idiomatic code per language.

| Emitter        | Output                                   | Runtime validation     | Conformance*         |
| -------------- | ---------------------------------------- | ---------------------- | -------------------- |
| **Zod**        | Zod schemas + TypeScript types           | Yes (full)             | **100%** (1299/1299) |
| **Pydantic**   | `@pydantic` dataclasses (Pydantic v2)    | Yes                    | **100%** (1299/1299) |
| **Swift**      | `Codable` structs + validating wrappers  | Yes (decode-time)      | **100%** (1299/1299) |
| **Kotlin**     | `@Serializable` data classes             | Yes (decode-time)      | **100%** (1299/1299) |
| **Rust**       | serde structs + validating `Deserialize` | Yes (decode-time)      | **100%** (1299/1299) |
| **TypeScript** | Pure `.d.ts` type definitions            | No (compile-time only) | n/a                  |

\* The full official [JSON-Schema-Test-Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite) (draft 2020-12, 1,299 test cases — nothing skipped), measured under the standard draft 2020-12 profile where `format` is annotation-only. Format enforcement is this library's opt-in extension (on by default in the API/CLI; see `Converter::convert_with_options`).

Most schemas compile to fully native validators. Schemas using keywords whose semantics require evaluation-annotation tracking or cross-document reference resolution (`unevaluatedProperties`/`unevaluatedItems`, `$dynamicRef`/`$dynamicAnchor`, `$anchor`, nested `$id` scopes, remote `$ref`s, custom `$schema` dialects) are compiled instead to an embedded, self-contained draft 2020-12 mini-validator per language — same public type, same API, spec-complete semantics.

## Workspace

```
crates/
  json-schema-transformer/   Pure library. Emitters are opt-in cargo features.
  jst-cli/                   `jst` binary — convert schemas from the command line.
  conformance-gen/           Internal tool: generates conformance fixtures from the test suite.
conformance/
  ts/ python/ swift/ kotlin/ rust/ Native-language conformance harnesses (see Testing).
  generated/                 Generated fixtures + manifest.json (regenerable, gitignored).
  results/                   Per-language results JSON (regenerable, gitignored).
fixtures/
  JSON-Schema-Test-Suite/    Official test suite (git submodule).
scripts/test.ts              Orchestrates build → unit → generate → conformance.
```

## Library

Emitters are opt-in features so downstream users only compile what they need:

```toml
[dependencies]
json-schema-transformer = { version = "0.2", features = ["zod", "swift"] }
# or features = ["all-emitters"]
```

```rust
use json_schema_transformer::{transform, ZodEmitter};

let schema: serde_json::Value = serde_json::from_str(raw)?;
// Name: explicit argument, or the schema's root "title" when None.
// Neither present → Err(ConvertError::MissingName).
let code = transform(&schema, &ZodEmitter, Some("user-created"))?;
let code = transform(&schema, &ZodEmitter, None)?; // uses schema "title"
```

## CLI

Install with Homebrew (macOS and Linux):

```bash
brew install childishforces/tap/jst
```

Debian/Ubuntu packages (`jst_<version>_{amd64,arm64}.deb`) and prebuilt
binaries for macOS and Linux are attached to each
[GitHub Release](https://github.com/ChildishForces/json-schema-transformer/releases),
or build from source:

```bash
cargo build --release -p jst-cli
```

Usage — the CLI runs in one of two modes:

**Single-file mode** (one schema → `--out` or stdout): the generated module is fully self-contained, with all runtime helpers inlined.

```bash
# Single target to stdout
jst schema.json --target zod

# To a specific file
jst schema.json --target swift --out OrderItem.swift

# From stdin, custom naming
echo '{"type":"string"}' | jst - --target swift --name my-type
```

**Collection mode** (`--out-dir`, multiple schemas, or a directory input): one output file per schema plus a shared helpers file at the output root containing exactly the union of helpers the emitted modules need — nothing more. Directory inputs are expanded recursively (`**/*.json`) and their structure is mirrored into the output directory; nested modules reference the root helpers file with depth-aware paths.

```bash
# All targets into a directory (collection mode: a shared helpers file is
# written when the generated modules need one)
jst schema.json --out-dir ./generated

# Many schemas, mirrored tree:
#   schemas/orders/item.json  → generated/orders/Item.zod.ts
#   schemas/users/profile.json → generated/users/Profile.zod.ts
#   + generated/jst-helpers.ts (tailored to what the modules use)
jst schemas/ --target zod --out-dir ./generated

# Mix files and directories; override the helpers file name (single target)
jst extra.json schemas/ --target zod --out-dir ./generated --helpers-file utils.ts
```

The generated root type name is the PascalCased name, resolved in order: `--name` argument (single-file mode), the schema's root `title`, then the input file's stem (library callers get `ConvertError::MissingName` instead of the file-stem fallback). Collection-mode output file stems follow each language's convention: `snake_case` for Python and Rust, `PascalCase` otherwise.

The shared helpers file — `jst-helpers.ts` / `jst_helpers.py` / `JstHelpers.swift` / `JstHelpers.kt` / `jst_helpers.rs` — is referenced via imports in TypeScript/Python, same-module internal declarations in Swift/Kotlin, and `use super::…::jst_helpers::*;` in Rust (mount the generated tree as a module hierarchy mirroring the directories, with `jst_helpers.rs` a sibling of the root-level files; generated Rust depends on `serde` + `serde_json` + `regex`). Library callers use `CollectionSession` (accumulates helper needs across `emit` calls, then `helpers()` yields the tailored file) or `Emitter::emit_collecting` / `Emitter::helpers_content_for` directly; `EmitOptions { helpers }` carries the helpers file name and the per-module directory prefix.

### Re-validation and mutability

Generated types validate at decode time, and every validating target also exposes an **explicit validation API** for values that are constructed or mutated in code rather than decoded:

- **Swift** — every type conforms to a generated `Validatable` protocol: `try value.validate()`, `value.isValid`, and `try value.validatedJSONData()` (validate, then encode). Typed structs also get a throwing memberwise initializer (`try OrderItem(id: "a", quantity: 2)`) that rejects invalid values at construction.
- **Kotlin** — every generated class implements `_JstValidatable` (`validate()` throws on invalid, `toValidatedJson()`), and object-shaped wrapper types get a typed companion constructor (`OrderItem(id = "a", quantity = 1)`).
- **Rust** — every type has `validate(&self) -> Result<(), String>` and `to_validated_json(&self) -> Result<String, String>`; fields are `pub`, so construction and mutation are always possible.
- **Pydantic** — dataclasses are mutable; with `--mutable`, `validate_assignment=True` is emitted so invalid attribute assignment raises immediately. Explicit re-validation: `TypeAdapter(OrderItem).validate_python(...)`.
- **Zod** — parse outputs are plain objects; re-validate by re-running `OrderItemSchema.parse(value)`.

`--mutable` switches stored properties from Swift `let` / Kotlin `val` to `var` (Rust, Python, and TypeScript outputs are already mutable, so it changes nothing there beyond the Pydantic config above). Validation APIs are emitted regardless of the flag.

One caveat by design: **plain encoding does not validate** (`JSONEncoder().encode`, kotlinx `encodeToString`, serde serialization). `validate()` is implemented by round-tripping through the validating decoder, so hooking the raw encode path would recurse; use the `validatedJSONData()` / `toValidatedJson()` / `to_validated_json()` helpers when you need serialize-only-if-valid.

**Error reporting is complete, not first-failure**: every validating target reports the full list of failed constraints with paths — Zod natively (`ZodError.issues`), Pydantic natively (`ValidationError.errors()`), and the generated Swift/Kotlin/Rust types via a `JstIssue { path, message }` list (JSON-Pointer paths, e.g. `/quantity: must be >= 1; /tags/1: duplicate item; /extra: unexpected property`) carried by `JstError` (Rust), `JstValidationError` (Swift), and `JstValidationException` (Kotlin). Composition keywords (`anyOf`/`oneOf`) report a single match/no-match verdict, and type mismatches short-circuit only their own subtree.

Consuming the issues (Kotlin shown — decode, `validate()` and `toValidatedJson()` all throw the same exception; in Rust, `validate()` / `to_validated_json()` return `Result<_, JstError>` with the same `issues` vector, and serde decode errors surface the joined message):

```kotlin
try {
    Json.decodeFromString<OrderItem>(payload)
} catch (e: JstValidationException) {   // extends SerializationException — existing catch sites keep working
    for ((path, message) in e.issues) println("$path: $message")
    // /quantity: must be >= 1
    // /tags/1: duplicate item
    // /extra: unexpected property
}
```

`e.message` joins all issues (`"; "`-separated, root-level issues message-only). In collection mode the Kotlin error classes are shared from `jst.helpers`; in single-file mode they are declared per file as `_{RootType}JstIssue` / `_{RootType}JstValidationException`. Plain `@Serializable data class` shapes with no constraint checks keep kotlinx's native errors.

## Testing

```bash
git submodule update --init  # first time: fetch the JSON Schema test suite
bun scripts/test.ts          # build, unit, integration, fixtures, all 5 conformance suites
bun scripts/test.ts --quick  # build + unit tests only
bun run test:integration     # per-harness integration tests only (jst CLI → generated code → native run)
```

Each harness also has an integration test (`conformance/<lang>/integration.test.ts`) that drives the real `jst` CLI over a sample schema (name from `title`, collection mode) and executes the generated code natively, asserting accept/reject behavior.

Conformance testing runs the full official test suite **natively in each output language** — each harness compiles/loads all 383 generated fixture modules once and executes all 1,299 cases in a single process:

| Suite    | Command                         | Approx. time                                                             |
| -------- | ------------------------------- | ------------------------------------------------------------------------ |
| Zod      | `bun run conformance/ts/run.ts` | < 0.3s                                                                   |
| Pydantic | `conformance/python/run.sh`     | ~1s                                                                      |
| Swift    | `bun conformance/swift/run.ts`  | ~7s (one `swiftc` batch compile)                                         |
| Kotlin   | `bun conformance/kotlin/run.ts` | ~25s (one `kotlinc` batch compile)                                       |
| Rust     | `bun conformance/rust/run.ts`   | ~1min first run (cargo deps), then one incremental `cargo` batch compile |

Fixtures are produced by `conformance-gen`, which walks `fixtures/JSON-Schema-Test-Suite/tests/draft2020-12`, converts every test group through every emitter, and writes `conformance/generated/manifest.json` describing each group (type name, generated file per language, expected validity per test). Generation errors are recorded in the manifest and counted as failures by the harnesses — no test is silently skipped.

Requirements per harness: bun (Zod), python3 + venv (Pydantic — created automatically at `conformance/python/.venv`), swiftc (Swift), kotlinc + java (Kotlin — serialization jars vendored in `conformance/kotlin/libs/`), cargo (Rust — serde/serde_json/regex from crates.io, locked via `conformance/rust/Cargo.lock.template`).

## Architecture

```
JSON Schema ──▶ Converter (input.rs) ──▶ SchemaIr (ir.rs) ──▶ Emitter (emit/*.rs) ──▶ code
```

- **`input.rs`** — Draft 2020-12 parser: `$ref`/`$defs` resolution with cycle detection, `allOf` object merging, `anyOf`/`oneOf` unions, `if`/`then`/`else`, typeless schemas via type-guarded constraints, topological ordering of definitions.
- **`ir.rs`** — target-agnostic schema IR: primitives with constraints, objects/arrays/tuples/records, unions/intersections, literals/enums, refs, refinement wrappers.
- **`emit/`** — one emitter module per target implementing the `Emitter` trait (`emit/zod/`, `emit/swift/`, ...). Each emitter directory contains its Rust code (`mod.rs`) plus its runtime helper sources as real files in their own language (`jsi_validator.ts`, `email_address.swift`, `jsi_eq.kt`, ...), embedded via `include_str!`.
