use crate::ir::*;
use crate::util::{escape_string, to_pascal_case};
use super::{EmitOptions, Emitter, module_header, needs_quoting};

pub struct TypeScriptEmitter;

impl Emitter for TypeScriptEmitter {
    fn extension(&self) -> &str {
        "d.ts"
    }

    fn emit_with_options(
        &self,
        converted: &ConvertedSchema,
        name: &str,
        namespace: &str,
        version: u32,
        _options: &EmitOptions,
    ) -> String {
        let pascal = to_pascal_case(name);
        let mut out = module_header(namespace, name, version);
        out.push('\n');

        // Emit definitions
        for def in &converted.defs {
            out.push_str(&emit_type_def(&def.name, &def.schema, &converted.defs));
            out.push('\n');
        }

        // Emit root type
        let root_name = format!("{pascal}V{version}Data");
        out.push_str(&emit_type_def(&root_name, &converted.root, &converted.defs));

        out
    }
}

fn emit_type_def(name: &str, ir: &SchemaIr, defs: &[DefEntry]) -> String {
    match ir {
        SchemaIr::Object(obj) => {
            let fields: Vec<String> = obj
                .fields
                .iter()
                .map(|(k, f)| {
                    let opt = if f.required { "" } else { "?" };
                    let key = if needs_quoting(k) {
                        format!("\"{}\"", escape_string(k))
                    } else {
                        k.clone()
                    };
                    format!("  {key}{opt}: {};", ts_type_indented(&f.schema, defs, 1))
                })
                .collect();

            if fields.is_empty() {
                format!("export interface {name} {{}}\n")
            } else {
                format!("export interface {name} {{\n{}\n}}\n", fields.join("\n"))
            }
        }
        _ => {
            format!("export type {name} = {};\n", ts_type_indented(ir, defs, 0))
        }
    }
}

/// Generate a TypeScript type string with indentation for readability.
/// `depth` tracks nesting level for inline objects.
fn ts_type_indented(ir: &SchemaIr, defs: &[DefEntry], depth: usize) -> String {
    match ir {
        SchemaIr::String(_) => "string".to_string(),
        SchemaIr::Number(_) | SchemaIr::Integer(_) => "number".to_string(),
        SchemaIr::Boolean => "boolean".to_string(),
        SchemaIr::Null => "null".to_string(),
        SchemaIr::Any | SchemaIr::Unknown | SchemaIr::Interpreted { .. } => "unknown".to_string(),
        SchemaIr::Never => "never".to_string(),

        SchemaIr::Literal(lit) => match lit {
            LiteralValue::String(s) => format!("\"{}\"", escape_string(s)),
            LiteralValue::Number(n) => format!("{n}"),
            LiteralValue::Integer(i) => format!("{i}"),
            LiteralValue::Bool(b) => format!("{b}"),
            LiteralValue::Null => "null".to_string(),
        },

        SchemaIr::ConstEqual(val) => {
            serde_json::to_string(val).unwrap_or_else(|_| "unknown".to_string())
        }

        SchemaIr::Enum(cases) => {
            let parts: Vec<String> = cases
                .iter()
                .map(|c| format!("\"{}\"", escape_string(c)))
                .collect();
            parts.join(" | ")
        }

        SchemaIr::MixedEnum(lits) => {
            let parts: Vec<String> = lits
                .iter()
                .map(|lit| ts_type_indented(&SchemaIr::Literal(lit.clone()), defs, depth))
                .collect();
            parts.join(" | ")
        }

        SchemaIr::ComplexEnum(values) => {
            let parts: Vec<String> = values
                .iter()
                .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "unknown".to_string()))
                .collect();
            parts.join(" | ")
        }

        SchemaIr::Array(arr) => {
            let inner = ts_type_indented(&arr.items, defs, depth);
            if inner.contains('|') || inner.contains('&') {
                format!("({inner})[]")
            } else {
                format!("{inner}[]")
            }
        }

        SchemaIr::Tuple(tuple) => {
            let parts: Vec<String> = tuple.items.iter().map(|i| ts_type_indented(i, defs, depth)).collect();
            if let Some(rest) = &tuple.rest {
                let rest_type = ts_type_indented(rest, defs, depth);
                format!("[{}...{}[]]",
                    if parts.is_empty() { String::new() } else { format!("{}, ", parts.join(", ")) },
                    rest_type
                )
            } else {
                format!("[{}]", parts.join(", "))
            }
        }

        SchemaIr::Object(obj) => {
            let indent = "  ".repeat(depth + 1);
            let close_indent = "  ".repeat(depth);
            let fields: Vec<String> = obj
                .fields
                .iter()
                .map(|(k, f)| {
                    let opt = if f.required { "" } else { "?" };
                    let key = if needs_quoting(k) {
                        format!("\"{}\"", escape_string(k))
                    } else {
                        k.clone()
                    };
                    format!("{indent}{key}{opt}: {};", ts_type_indented(&f.schema, defs, depth + 1))
                })
                .collect();
            if fields.is_empty() {
                "Record<string, unknown>".to_string()
            } else {
                format!("{{\n{}\n{close_indent}}}", fields.join("\n"))
            }
        }

        SchemaIr::Record(rec) => {
            format!("Record<string, {}>", ts_type_indented(&rec.value, defs, depth))
        }

        SchemaIr::Union(members) | SchemaIr::ExclusiveUnion(members) => {
            let parts: Vec<String> = members.iter().map(|m| ts_type_indented(m, defs, depth)).collect();
            parts.join(" | ")
        }

        SchemaIr::Intersection(members) => {
            let parts: Vec<String> = members.iter().map(|m| ts_type_indented(m, defs, depth)).collect();
            parts.join(" & ")
        }

        SchemaIr::Nullable(inner) => format!("{} | null", ts_type_indented(inner, defs, depth)),
        SchemaIr::Optional(inner) => format!("{} | undefined", ts_type_indented(inner, defs, depth)),
        SchemaIr::Default(inner, _) => ts_type_indented(inner, defs, depth),
        SchemaIr::Describe(inner, _) => ts_type_indented(inner, defs, depth),
        SchemaIr::Not { base, .. } => ts_type_indented(base, defs, depth),
        SchemaIr::Conditional { base, .. } => ts_type_indented(base, defs, depth),
        SchemaIr::TypeGuarded(_) => "unknown".to_string(),

        SchemaIr::Ref(name) | SchemaIr::Lazy(name) => name.clone(),
    }
}

/// Public alias for use by other emitters (e.g., Zod's ts_type generation).
pub fn ts_type(ir: &SchemaIr, defs: &[DefEntry]) -> String {
    ts_type_indented(ir, defs, 0)
}
