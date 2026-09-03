# json-schema-transformer

Transform JSON Schema (Draft 2020-12) into native types and validation schemas for multiple languages. A single Rust core parses any JSON Schema into a target-agnostic intermediate representation (`SchemaIr`), then pluggable emitters generate idiomatic code per language.

| Emitter        | Output                                   | Runtime validation     | Conformance* |
| -------------- | ---------------------------------------- | ---------------------- | ------------ |
| **Zod**        | Zod schemas + TypeScript types           | Yes (full)             | 88.8%        |
| **Pydantic**   | `@pydantic` dataclasses (Pydantic v2)    | Yes                    | 89.7%        |
| **Swift**      | `Codable` structs + validating wrappers  | Yes (decode-time)      | 89.5%        |
| **Kotlin**     | `@Serializable` data classes             | Yes (decode-time)      | 86.5%        |
| **TypeScript** | Pure `.d.ts` type definitions            | No (compile-time only) | n/a          |

\* Share of the official [JSON-Schema-Test-Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite) (draft 2020-12, 1,299 test cases — nothing skipped) passing as of the current baseline. The goal is 100%; the biggest shared gaps are `unevaluatedProperties`/`unevaluatedItems`, `$dynamicRef`/`$dynamicAnchor`, `$anchor`, and remote `$ref`s.

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
JSON-Schema-Test-Suite/      Official test suite (git submodule).
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
let code = transform(&schema, &ZodEmitter, "user-created", "users", 1)?;
```

## CLI

```bash
cargo build --release -p jst-cli

# Single target to stdout
jst schema.json --target zod

# All targets into a directory
jst schema.json --out-dir ./generated

# From stdin, custom naming
echo '{"type":"string"}' | jst - --target swift --name my-type --namespace api --schema-version 2
```

Generated root types are named `{PascalName}V{version}Data` to keep multiple schema versions collision-free.

## Testing

```bash
git submodule update --init  # first time: fetch the JSON Schema test suite
bun scripts/test.ts          # everything: build, unit, fixtures, all 4 conformance suites (~20s)
bun scripts/test.ts --quick  # build + unit tests only
```

Conformance testing runs the full official test suite **natively in each output language** — each harness compiles/loads all 383 generated fixture modules once and executes all 1,299 cases in a single process:

| Suite    | Command                          | Approx. time |
| -------- | -------------------------------- | ------------ |
| Zod      | `bun run conformance/ts/run.ts`  | < 0.1s       |
| Pydantic | `conformance/python/run.sh`      | ~0.5s        |
| Swift    | `conformance/swift/run.sh`       | ~6s (one `swiftc` batch compile) |
| Kotlin   | `conformance/kotlin/run.sh`      | ~11s (one `kotlinc` batch compile) |

Fixtures are produced by `conformance-gen`, which walks `JSON-Schema-Test-Suite/tests/draft2020-12`, converts every test group through every emitter, and writes `conformance/generated/manifest.json` describing each group (type name, generated file per language, expected validity per test). Generation errors are recorded in the manifest and counted as failures by the harnesses — no test is silently skipped.

Requirements per harness: bun (Zod), python3 + venv (Pydantic — created automatically at `conformance/python/.venv`), swiftc (Swift), kotlinc + java (Kotlin — serialization jars vendored in `conformance/kotlin/libs/`).

## Architecture

```
JSON Schema ──▶ Converter (input.rs) ──▶ SchemaIr (ir.rs) ──▶ Emitter (emit/*.rs) ──▶ code
```

- **`input.rs`** — Draft 2020-12 parser: `$ref`/`$defs` resolution with cycle detection, `allOf` object merging, `anyOf`/`oneOf` unions, `if`/`then`/`else`, typeless schemas via type-guarded constraints, topological ordering of definitions.
- **`ir.rs`** — target-agnostic schema IR: primitives with constraints, objects/arrays/tuples/records, unions/intersections, literals/enums, refs, refinement wrappers.
- **`emit/`** — one emitter per target implementing the `Emitter` trait; shared helpers in `emit/mod.rs`.
