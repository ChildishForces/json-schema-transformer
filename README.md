# json-schema-transformer

Transform JSON Schema (Draft 2020-12) into native types and validation schemas for multiple languages. A single Rust core parses any JSON Schema into a target-agnostic intermediate representation (`SchemaIr`), then pluggable emitters generate idiomatic code per language.

| Emitter        | Output                                  | Runtime validation     | Conformance*         |
| -------------- | --------------------------------------- | ---------------------- | -------------------- |
| **Zod**        | Zod schemas + TypeScript types          | Yes (full)             | **100%** (1299/1299) |
| **Pydantic**   | `@pydantic` dataclasses (Pydantic v2)   | Yes                    | **100%** (1299/1299) |
| **Swift**      | `Codable` structs + validating wrappers | Yes (decode-time)      | **100%** (1299/1299) |
| **Kotlin**     | `@Serializable` data classes            | Yes (decode-time)      | **100%** (1299/1299) |
| **TypeScript** | Pure `.d.ts` type definitions           | No (compile-time only) | n/a                  |

\* The full official [JSON-Schema-Test-Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite) (draft 2020-12, 1,299 test cases — nothing skipped), measured under the standard draft 2020-12 profile where `format` is annotation-only. Format enforcement is this library's opt-in extension (on by default in the API/CLI; see `Converter::convert_with_options`).

Most schemas compile to fully native validators. Schemas using keywords whose semantics require evaluation-annotation tracking or cross-document reference resolution (`unevaluatedProperties`/`unevaluatedItems`, `$dynamicRef`/`$dynamicAnchor`, `$anchor`, nested `$id` scopes, remote `$ref`s, custom `$schema` dialects) are compiled instead to an embedded, self-contained draft 2020-12 mini-validator per language — same public type, same API, spec-complete semantics.

## Workspace

```
crates/
  json-schema-transformer/   Pure library. Emitters are opt-in cargo features.
  jst-cli/                   `jst` binary — convert schemas from the command line.
  conformance-gen/           Internal tool: generates conformance fixtures from the test suite.
conformance/
  ts/ python/ swift/ kotlin/ Native-language conformance harnesses (see Testing).
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
json-schema-transformer = { version = "0.1", features = ["zod", "swift"] }
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

```bash
cargo build --release -p jst-cli

# Single target to stdout
jst schema.json --target zod

# All targets into a directory
jst schema.json --out-dir ./generated

# From stdin, custom naming
echo '{"type":"string"}' | jst - --target swift --name my-type

# Shared helpers: utilities (validators, wrapper types) go to a separate
# companion file instead of being inlined into each schema module
jst schema.json --target zod --out-dir ./generated --helpers-file            # per-language default name
jst schema.json --target zod --out-dir ./generated --helpers-file utils.ts   # custom name
```

The generated root type name is the PascalCased name, resolved in order: `--name` argument, the schema's root `title`, then the input file's stem (library callers get `ConvertError::MissingName` instead of the file-stem fallback).

By default every generated module is fully self-contained. With `--helpers-file` (library: `EmitOptions { helpers_file }` and `Emitter::helpers_content()`), shared utilities are emitted once into a companion file — `jst-helpers.ts` / `jst_helpers.py` / `JstHelpers.swift` / `JstHelpers.kt` — and schema modules reference them (imports in TypeScript/Python, same-module internal declarations in Swift/Kotlin). This keeps schema files free of utility noise and avoids duplicated declarations when many generated files are compiled together.

## Testing

```bash
git submodule update --init  # first time: fetch the JSON Schema test suite
bun scripts/test.ts          # build, unit, integration, fixtures, all 4 conformance suites (~50s)
bun scripts/test.ts --quick  # build + unit tests only
bun run test:integration     # per-harness integration tests only (jst CLI → generated code → native run)
```

Each harness also has an integration test (`conformance/<lang>/integration.test.ts`) that drives the real `jst` CLI over a sample schema (name from `title`, shared-helpers mode) and executes the generated code natively, asserting accept/reject behavior.

Conformance testing runs the full official test suite **natively in each output language** — each harness compiles/loads all 383 generated fixture modules once and executes all 1,299 cases in a single process:

| Suite    | Command                         | Approx. time                       |
| -------- | ------------------------------- | ---------------------------------- |
| Zod      | `bun run conformance/ts/run.ts` | < 0.3s                             |
| Pydantic | `conformance/python/run.sh`     | ~1s                                |
| Swift    | `bun conformance/swift/run.ts`  | ~7s (one `swiftc` batch compile)   |
| Kotlin   | `bun conformance/kotlin/run.ts` | ~25s (one `kotlinc` batch compile) |

Fixtures are produced by `conformance-gen`, which walks `fixtures/JSON-Schema-Test-Suite/tests/draft2020-12`, converts every test group through every emitter, and writes `conformance/generated/manifest.json` describing each group (type name, generated file per language, expected validity per test). Generation errors are recorded in the manifest and counted as failures by the harnesses — no test is silently skipped.

Requirements per harness: bun (Zod), python3 + venv (Pydantic — created automatically at `conformance/python/.venv`), swiftc (Swift), kotlinc + java (Kotlin — serialization jars vendored in `conformance/kotlin/libs/`).

## Architecture

```
JSON Schema ──▶ Converter (input.rs) ──▶ SchemaIr (ir.rs) ──▶ Emitter (emit/*.rs) ──▶ code
```

- **`input.rs`** — Draft 2020-12 parser: `$ref`/`$defs` resolution with cycle detection, `allOf` object merging, `anyOf`/`oneOf` unions, `if`/`then`/`else`, typeless schemas via type-guarded constraints, topological ordering of definitions.
- **`ir.rs`** — target-agnostic schema IR: primitives with constraints, objects/arrays/tuples/records, unions/intersections, literals/enums, refs, refinement wrappers.
- **`emit/`** — one emitter module per target implementing the `Emitter` trait (`emit/zod/`, `emit/swift/`, ...). Each emitter directory contains its Rust code (`mod.rs`) plus its runtime helper sources as real files in their own language (`jsi_validator.ts`, `email_address.swift`, `jsi_eq.kt`, ...), embedded via `include_str!`.
