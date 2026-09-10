# Design note: property wrappers for Swift decode-time validation

**Status:** assessed, not adopted (2026-09)

## TL;DR

The suggestion was that the Swift output "should probably use property wrappers as a means of
validating at decode time." The premise doesn't quite hold: **the Swift emitter already validates
at decode time** — every constrained type gets a custom `init(from: Decoder)` that throws
`ValidationError`, and the output passes 100% of the official JSON Schema Test Suite
(1,299/1,299, see [`conformance/results/swift.json`](../conformance/results/swift.json)).

Property wrappers would therefore be a _re-skin of the mechanism_, not a new capability — and a
partial one. Swift's `Codable` synthesis cannot see property-wrapper attribute arguments during
decoding, so constraint parameters would have to be lifted into generated phantom types; roughly
half of the JSON Schema keyword surface can't be expressed at the property level at all; and
wrappers force the generated API from `let` to `var`. **Recommendation: keep the current
`init(from:)` strategy.** An opt-in wrapper mode is sketched at the end if the declarative
property-site style is ever wanted.

## How validation works today

Generation pipeline: JSON Schema → IR → the Swift emitter
([`crates/json-schema-transformer/src/emit/swift/mod.rs`](../crates/json-schema-transformer/src/emit/swift/mod.rs)).
For this schema:

```json
{
  "title": "Order Item",
  "type": "object",
  "properties": {
    "id": { "type": "string" },
    "quantity": { "type": "integer", "minimum": 1 },
    "tags": { "type": "array", "items": { "type": "string" }, "uniqueItems": true }
  },
  "required": ["id", "quantity"],
  "additionalProperties": false
}
```

`jst sample.json --target swift` emits (trimmed):

```swift
struct OrderItem: Codable {
    let id: String
    let quantity: Int
    let tags: [String]?

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        // additionalProperties: false — reject unknown keys
        for key in allKeysContainer.allKeys where !knownKeys.contains(key.stringValue) {
            throw ValidationError.outOfRange(key.stringValue, "unexpected property")
        }
        id = try container.decode(String.self, forKey: .id)
        quantity = try container.decode(Int.self, forKey: .quantity)
        if !(quantity >= 1) { throw ValidationError.outOfRange("quantity", ">= 1") }
        tags = try container.decodeIfPresent([String].self, forKey: .tags)
        // uniqueItems check on tags ...
    }
}
```

So `JSONDecoder().decode(OrderItem.self, from: data)` _is_ the validation step — invalid
documents throw before a value can exist. On top of that:

- **Formats** are enforced by self-validating types (`EmailAddress`, `IPv4Address`,
  `HostnameString`, `TimeString`, `DurationString`, `Base64String`, plus `Date`/`URL`/`UUID`),
  emitted inline in single-file mode or once into `JstHelpers.swift` in collection mode.
- **Compositions** (`oneOf`/`anyOf`/`allOf`/`not`/`if-then-else`) are checked by attempting
  member decodes and counting matches.
- **Draft 2020-12 long tail** (`unevaluatedProperties`, `$dynamicRef`, …) is handled by an
  embedded mini-interpreter (`jsi_validator.swift`).

Invalid values are unrepresentable after decode — which is the actual goal behind the
property-wrapper suggestion.

## The core limitation: wrapper arguments are invisible to `Codable`

The natural design people imagine is:

```swift
struct OrderItem: Codable {
    @Validated(minimum: 1) var quantity: Int
}
```

This cannot work. When `Codable` synthesis encounters a wrapped property, it decodes the
_wrapper storage_ by calling the wrapper's own decoding initializer:

```swift
_quantity = try container.decode(Validated<Int>.self, forKey: .quantity)
// i.e. Validated(from: decoder) — no arguments
```

The attribute arguments `(minimum: 1)` desugar into the _memberwise_ initializer
(`Validated(wrappedValue:minimum:)`) and are simply never invoked on the decode path. Inside
`Validated.init(from:)` there is no way to know which constraints this particular property
carries.

The only information available at decode time is the **type**, so constraints must be encoded in
the type — one generated phantom "spec" type per constrained property:

```swift
// generated per property
enum OrderItem_quantity_Spec: IntSpec { static let minimum: Int? = 1 }

struct OrderItem: Codable {
    var id: String
    @Constrained<OrderItem_quantity_Spec> var quantity: Int
}

// once, in JstHelpers.swift
@propertyWrapper
struct Constrained<S: IntSpec>: Codable {
    var wrappedValue: Int
    init(from decoder: Decoder) throws {
        let v = try decoder.singleValueContainer().decode(Int.self)
        try S.validate(v)                    // throws ValidationError
        wrappedValue = v
    }
    func encode(to encoder: Encoder) throws { try wrappedValue.encode(to: encoder) }
}
```

That is workable, but note what it costs: every constrained property now generates a companion
spec type, and the constraint values live _away_ from the property they describe — arguably less
readable than the two-line check in `init(from:)` it replaces. Two further mechanical taxes:

- **Optionals break by default.** Synthesis uses `decode`, not `decodeIfPresent`, for wrapped
  properties, so an absent optional key throws. Fixing it requires `KeyedDecodingContainer`
  overloads per wrapper shape (the well-known BetterCodable workaround).
- **Encoding must be forwarded** in each wrapper (as above) or output gains a nesting level.

## What wrappers can never express

Property wrappers attach to stored properties. A large share of the keyword surface — all of it
currently passing conformance — has no property to attach to:

| Category                      | Keywords                                                                                                                                                         |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Root-level non-object schemas | `{"type": "string", "minLength": 2}` at the document root (see [`conformance/generated/swift/MinimumG0.swift`](../conformance/generated/swift/MinimumG0.swift))  |
| Compositions                  | `oneOf`, `anyOf`, `allOf`, `not`, `if`/`then`/`else`                                                                                                             |
| Object-level constraints      | `minProperties`, `maxProperties`, `additionalProperties`, `patternProperties`, `propertyNames`, `dependentRequired`, `dependentSchemas`, `unevaluatedProperties` |
| Array-shape constraints       | per-element schemas, `contains`/`minContains`/`maxContains`, `prefixItems`, `unevaluatedItems`                                                                   |
| Cross-document                | `$ref`, `$dynamicRef`, `$anchor`                                                                                                                                 |

All of these keep the existing `init(from:)` / interpreter machinery regardless. Property
wrappers can only ever be a partial layer over leaf constraints (`minimum`, `maximum`,
`multipleOf`, `minLength`, `maxLength`, `pattern`, `minItems`, `maxItems`, `uniqueItems`) — and
most structs would end up with _both_ mechanisms visible at once.

## API-surface costs

- Property wrappers require `var`; generated structs currently expose `let`. Consumers lose
  immutability guarantees on every constrained field.
- Post-decode mutation silently bypasses validation unless the wrapper's setter re-validates —
  and setters can't throw, so the options are trapping (`fatalError`) or clamping. Today's
  `let` fields sidestep the question entirely. _(Since addressed without wrappers: generated
  types now conform to `Validatable` — `validate() throws` / `validatedJSONData()` — with
  throwing memberwise initializers, and `--mutable` opts into `var` fields with explicit
  re-validation after mutation.)_
- Every constrained property adds a generated spec type to the file (or to `JstHelpers.swift`),
  growing output for schemas that lean on numeric/string constraints.

Note that the _good_ part of the wrapper idea — declarative, type-carried validation visible in
the struct definition — is already present where it pays off: the format types. `let email:
EmailAddress` gives the same "invalid values don't typecheck" ergonomics without any of the
`Codable`-synthesis friction, because it's a plain validating type rather than a wrapper.

## If someone wants it anyway: opt-in sketch

Should declarative property-site annotations become a real user request, the shape that fits
this codebase:

1. A `--swift-property-wrappers` CLI flag threading through `EmitOptions` to the Swift emitter;
   default output is unchanged.
2. Wrapper types (`Constrained<S: IntSpec>`, `<S: DoubleSpec>`, `<S: StringSpec>`,
   `<S: ArraySpec>`) plus the `KeyedDecodingContainer` optional-decoding overloads emitted into
   `JstHelpers.swift` (shared-helpers mode required, since the wrappers are generic and
   reusable).
3. Spec-type generation for exactly the leaf constraints listed above; every other keyword
   falls back to the existing `init(from:)` path in the same struct.
4. Acceptance bar: the conformance suite stays at 1,299/1,299 with the flag on
   (`conformance/swift/run.ts` re-run against flag-enabled fixtures), and the existing
   integration test gains a flag-enabled variant.

Until that request exists, the current design is smaller, covers the full spec, and validates at
decode time already — so it stands.
