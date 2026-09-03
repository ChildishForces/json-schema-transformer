pub mod emit;
pub mod input;
pub mod ir;
pub mod util;

pub use emit::Emitter;
#[cfg(feature = "kotlin")]
pub use emit::KotlinEmitter;
#[cfg(feature = "pydantic")]
pub use emit::PydanticEmitter;
#[cfg(feature = "swift")]
pub use emit::SwiftEmitter;
#[cfg(feature = "typescript")]
pub use emit::TypeScriptEmitter;
#[cfg(feature = "zod")]
pub use emit::ZodEmitter;
pub use input::{ConvertError, Converter};
pub use ir::{ConvertedSchema, SchemaIr};

/// Convert a JSON Schema using the given emitter strategy.
pub fn transform(
    schema: &serde_json::Value,
    emitter: &dyn Emitter,
    name: &str,
    namespace: &str,
    version: u32,
) -> Result<String, ConvertError> {
    let converted = Converter::convert(schema)?;
    Ok(emitter.emit(&converted, name, namespace, version))
}

/// Convert a JSON Schema to a complete Zod TypeScript module string.
#[cfg(feature = "zod")]
pub fn json_schema_to_zod_module(
    schema: &serde_json::Value,
    name: &str,
    namespace: &str,
    version: u32,
) -> Result<String, ConvertError> {
    transform(schema, &ZodEmitter, name, namespace, version)
}

/// Convert a JSON Schema to TypeScript type definitions (.d.ts).
#[cfg(feature = "typescript")]
pub fn json_schema_to_typescript(
    schema: &serde_json::Value,
    name: &str,
    namespace: &str,
    version: u32,
) -> Result<String, ConvertError> {
    transform(schema, &TypeScriptEmitter, name, namespace, version)
}

/// Convert a JSON Schema to a Python Pydantic model.
#[cfg(feature = "pydantic")]
pub fn json_schema_to_pydantic(
    schema: &serde_json::Value,
    name: &str,
    namespace: &str,
    version: u32,
) -> Result<String, ConvertError> {
    transform(schema, &PydanticEmitter, name, namespace, version)
}

/// Convert a JSON Schema to a Swift Codable struct.
#[cfg(feature = "swift")]
pub fn json_schema_to_swift(
    schema: &serde_json::Value,
    name: &str,
    namespace: &str,
    version: u32,
) -> Result<String, ConvertError> {
    transform(schema, &SwiftEmitter, name, namespace, version)
}

/// Convert a JSON Schema to a Kotlin data class.
#[cfg(feature = "kotlin")]
pub fn json_schema_to_kotlin(
    schema: &serde_json::Value,
    name: &str,
    namespace: &str,
    version: u32,
) -> Result<String, ConvertError> {
    transform(schema, &KotlinEmitter, name, namespace, version)
}

#[cfg(all(test, feature = "zod"))]
mod tests {
    use super::*;
    use emit::zod_for;
    use serde_json::json;

    fn zod(schema: serde_json::Value) -> String {
        let converted = Converter::convert(&schema).unwrap();
        zod_for(&converted.root, &converted.defs)
    }

    #[test]
    fn string_basic() {
        assert_eq!(zod(json!({"type": "string"})), "z.string()");
    }

    #[test]
    fn string_email() {
        assert_eq!(zod(json!({"type": "string", "format": "email"})), "z.string().email()");
    }

    #[test]
    fn string_min_max() {
        // Length checks count Unicode code points, so they use a superRefine
        let result = zod(json!({"type": "string", "minLength": 1, "maxLength": 50}));
        assert!(result.starts_with("z.string().superRefine("));
        assert!(result.contains("Array.from(v).length"));
        assert!(result.contains("n < 1"));
        assert!(result.contains("n > 50"));
    }

    #[test]
    fn string_pattern() {
        assert_eq!(
            zod(json!({"type": "string", "pattern": "^[a-z]+$"})),
            "z.string().regex(/^[a-z]+$/)"
        );
    }

    #[test]
    fn integer_minimum() {
        assert_eq!(
            zod(json!({"type": "integer", "minimum": 0})),
            "z.number().int().gte(0)"
        );
    }

    #[test]
    fn number_exclusive_max_draft7() {
        assert_eq!(
            zod(json!({"type": "number", "exclusiveMaximum": 100})),
            "z.number().lt(100)"
        );
    }

    #[test]
    fn number_exclusive_min_draft4() {
        assert_eq!(
            zod(json!({"type": "number", "exclusiveMinimum": true, "minimum": 0})),
            "z.number().gt(0)"
        );
    }

    #[test]
    fn nullable_type_array() {
        assert_eq!(
            zod(json!({"type": ["string", "null"]})),
            "z.string().nullable()"
        );
    }

    #[test]
    fn nullable_any_of() {
        assert_eq!(
            zod(json!({"anyOf": [{"type": "string"}, {"type": "null"}]})),
            "z.string().nullable()"
        );
    }

    #[test]
    fn union_any_of() {
        assert_eq!(
            zod(json!({"anyOf": [{"type": "string"}, {"type": "number"}]})),
            "z.union([z.string(), z.number()])"
        );
    }

    #[test]
    fn all_of_objects_merged() {
        let schema = json!({
            "allOf": [
                {"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]},
                {"type": "object", "properties": {"b": {"type": "number"}}, "required": ["b"]}
            ]
        });
        let result = zod(schema);
        assert!(result.contains("a: z.string()"));
        assert!(result.contains("b: z.number()"));
    }

    #[test]
    fn all_of_non_objects_intersection() {
        let schema = json!({
            "allOf": [
                {"type": "string"},
                {"type": "number"}
            ]
        });
        let result = zod(schema);
        assert!(result.starts_with("z.intersection("));
    }

    #[test]
    fn enum_strings() {
        assert_eq!(
            zod(json!({"enum": ["a", "b"]})),
            "z.enum([\"a\", \"b\"])"
        );
    }

    #[test]
    fn enum_mixed() {
        let result = zod(json!({"enum": [1, 2, 3]}));
        assert!(result.starts_with("z.union([z.literal("));
    }

    #[test]
    fn const_string() {
        assert_eq!(zod(json!({"const": "foo"})), "z.literal(\"foo\")");
    }

    #[test]
    fn object_required_and_optional() {
        let schema = json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "name": {"type": "string"}
            },
            "required": ["id"]
        });
        let result = zod(schema);
        assert!(result.contains("id: z.string()"));
        assert!(result.contains("name: z.string().optional()"));
    }

    #[test]
    fn object_additional_properties_false() {
        let schema = json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "additionalProperties": false
        });
        let result = zod(schema);
        assert!(result.ends_with(".strict()"));
    }

    #[test]
    fn array_of_strings() {
        assert_eq!(
            zod(json!({"type": "array", "items": {"type": "string"}})),
            "z.array(z.string())"
        );
    }

    #[test]
    fn tuple_prefix_items() {
        let result = zod(json!({
            "type": "array",
            "prefixItems": [{"type": "string"}, {"type": "number"}]
        }));
        assert!(result.contains("z.array(z.any()).superRefine"));
        assert!(result.contains("z.string()"));
        assert!(result.contains("z.number()"));
    }

    #[test]
    fn tuple_prefix_items_no_rest() {
        let result = zod(json!({
            "type": "array",
            "prefixItems": [{"type": "string"}, {"type": "number"}],
            "items": false
        }));
        assert!(result.contains("z.array(z.any()).superRefine"));
        assert!(result.contains("at most 2"));
    }

    #[test]
    fn boolean_schema_true() {
        assert_eq!(zod(json!(true)), "z.any()");
    }

    #[test]
    fn boolean_schema_false() {
        assert_eq!(zod(json!(false)), "z.never()");
    }

    #[test]
    fn const_object() {
        let result = zod(json!({"const": {"a": 1}}));
        assert!(result.contains("_deepEqual"));
    }

    #[test]
    fn not_keyword() {
        let result = zod(json!({"type": "string", "not": {"const": "forbidden"}}));
        assert!(result.contains("superRefine"));
        assert!(result.contains("z.string()"));
    }

    #[test]
    fn if_then_else() {
        let result = zod(json!({
            "type": "object",
            "if": {"properties": {"type": {"const": "a"}}},
            "then": {"required": ["name"]},
            "else": {"required": ["id"]}
        }));
        assert!(result.contains("ifResult"));
    }

    #[test]
    fn dependent_required() {
        let result = zod(json!({
            "type": "object",
            "dependentRequired": {"credit_card": ["billing_address"]}
        }));
        assert!(result.contains("billing_address"));
        assert!(result.contains("credit_card"));
    }

    #[test]
    fn unique_items() {
        let result = zod(json!({"type": "array", "items": {"type": "number"}, "uniqueItems": true}));
        assert!(result.contains("unique"));
    }

    #[test]
    fn contains_keyword() {
        let result = zod(json!({"type": "array", "contains": {"type": "number"}}));
        assert!(result.contains("matchCount"));
    }

    #[test]
    fn pattern_properties() {
        let result = zod(json!({
            "type": "object",
            "patternProperties": {"^x-": {"type": "string"}}
        }));
        assert!(result.contains("/^x-/"));
    }

    #[test]
    fn min_max_properties() {
        let result = zod(json!({
            "type": "object",
            "minProperties": 1,
            "maxProperties": 5
        }));
        assert!(result.contains("at least 1"));
        assert!(result.contains("at most 5"));
    }

    #[test]
    fn typeless_number_constraint() {
        let result = zod(json!({"minimum": 5}));
        assert!(result.contains("typeof val === \"number\""));
        assert!(result.contains("z.number()"));
    }

    #[test]
    fn description_modifier() {
        let result = zod(json!({"type": "string", "description": "A field"}));
        assert!(result.contains(".describe(\"A field\")"));
    }

    #[test]
    fn default_modifier() {
        let result = zod(json!({"type": "string", "default": "hello"}));
        assert!(result.contains(".default(\"hello\")"));
    }

    #[test]
    fn ref_to_definition() {
        let schema = json!({
            "$defs": {
                "Address": {
                    "type": "object",
                    "properties": {
                        "street": {"type": "string"}
                    },
                    "required": ["street"]
                }
            },
            "type": "object",
            "properties": {
                "address": {"$ref": "#/$defs/Address"}
            },
            "required": ["address"]
        });
        let module = json_schema_to_zod_module(&schema, "person", "test", 1).unwrap();
        assert!(module.contains("AddressSchema"));
        assert!(module.contains("PersonV1DataSchema"));
    }

    #[test]
    fn recursive_ref() {
        let schema = json!({
            "$defs": {
                "Node": {
                    "type": "object",
                    "properties": {
                        "value": {"type": "string"},
                        "children": {
                            "type": "array",
                            "items": {"$ref": "#/$defs/Node"}
                        }
                    },
                    "required": ["value"]
                }
            },
            "$ref": "#/$defs/Node"
        });
        let module = json_schema_to_zod_module(&schema, "tree", "test", 1).unwrap();
        assert!(module.contains("z.lazy("));
        assert!(module.contains("z.ZodType<"));
    }

    #[test]
    fn full_module_structure() {
        let schema = json!({"type": "object", "properties": {"id": {"type": "string", "format": "uuid"}}, "required": ["id"]});
        let module = json_schema_to_zod_module(&schema, "something-happened", "trade", 1).unwrap();
        assert!(module.contains("import { z } from \"zod\""));
        assert!(module.contains("export const SomethingHappenedV1DataSchema"));
        assert!(module.contains("export type SomethingHappenedV1Data"));
        assert!(module.contains("z.string().uuid()"));
    }
}
