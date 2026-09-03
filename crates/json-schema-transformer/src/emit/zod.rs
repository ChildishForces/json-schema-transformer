use crate::ir::*;
use crate::util::{escape_string, to_pascal_case};
use super::{Emitter, module_header, needs_quoting};

pub struct ZodEmitter;

impl Emitter for ZodEmitter {
    fn extension(&self) -> &str {
        "zod.ts"
    }

    fn emit(
        &self,
        converted: &ConvertedSchema,
        name: &str,
        namespace: &str,
        version: u32,
    ) -> String {
        emit_module(converted, name, namespace, version)
    }
}

const STABLE_STRINGIFY_HELPER: &str = r#"function _stableStringify(v: unknown): string {
  if (v === null || typeof v !== "object") return JSON.stringify(v);
  if (Array.isArray(v)) return "[" + v.map(_stableStringify).join(",") + "]";
  const obj = v as Record<string, unknown>;
  const keys = Object.keys(obj).sort();
  return "{" + keys.map(k => JSON.stringify(k) + ":" + _stableStringify(obj[k])).join(",") + "}";
}"#;

const DEEP_EQUAL_HELPER: &str = r#"function _deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null || a === undefined || b === undefined) return false;
  if (typeof a !== typeof b) return false;
  if (typeof a !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    const bArr = b as unknown[];
    if (a.length !== bArr.length) return false;
    return a.every((v, i) => _deepEqual(v, bArr[i]));
  }
  const aObj = a as Record<string, unknown>;
  const bObj = b as Record<string, unknown>;
  const aKeys = Object.keys(aObj);
  const bKeys = Object.keys(bObj);
  if (aKeys.length !== bKeys.length) return false;
  return aKeys.every(k => k in bObj && _deepEqual(aObj[k], bObj[k]));
}"#;

pub fn emit_module(
    converted: &ConvertedSchema,
    name: &str,
    namespace: &str,
    version: u32,
) -> String {
    let pascal = to_pascal_case(name);
    let mut out = String::new();

    out.push_str(&module_header(namespace, name, version));
    out.push('\n');
    out.push_str("import { z } from \"zod\";\n");

    // Compute helper needs by walking the IR
    let ir_needs_deep_equal = needs_deep_equal(&converted.root)
        || converted.defs.iter().any(|d| needs_deep_equal(&d.schema));
    let ir_needs_stable_stringify = has_unique_items(&converted.root)
        || converted.defs.iter().any(|d| has_unique_items(&d.schema));

    if ir_needs_deep_equal || ir_needs_stable_stringify {
        out.push_str("\n// --- Helpers ---\n");
    }
    if ir_needs_stable_stringify {
        out.push_str(STABLE_STRINGIFY_HELPER);
        out.push('\n');
    }
    if ir_needs_deep_equal {
        out.push_str(DEEP_EQUAL_HELPER);
        out.push('\n');
    }

    if !converted.defs.is_empty() {
        out.push_str("\n// --- Definitions ---\n");
        for def in &converted.defs {
            if def.is_recursive {
                let type_str = zod_ts_type(&def.schema, &converted.defs);
                out.push_str(&format!(
                    "type {name} = {type_str};\n",
                    name = def.name,
                    type_str = type_str
                ));
                out.push_str(&format!(
                    "const {name}Schema: z.ZodType<{name}> = z.lazy(() => {zod});\n",
                    name = def.name,
                    zod = zod_for(&def.schema, &converted.defs)
                ));
            } else {
                out.push_str(&format!(
                    "const {name}Schema = {zod};\n",
                    name = def.name,
                    zod = zod_for(&def.schema, &converted.defs)
                ));
                out.push_str(&format!(
                    "type {name} = z.infer<typeof {name}Schema>;\n",
                    name = def.name
                ));
            }
            out.push('\n');
        }
    }

    out.push_str("// --- Root schema ---\n");
    out.push_str(&format!(
        "export const {pascal}V{version}DataSchema = {zod};\n",
        pascal = pascal,
        version = version,
        zod = zod_for(&converted.root, &converted.defs)
    ));
    out.push_str(&format!(
        "export type {pascal}V{version}Data = z.infer<typeof {pascal}V{version}DataSchema>;\n",
        pascal = pascal,
        version = version
    ));

    out
}

pub fn zod_for(ir: &SchemaIr, defs: &[DefEntry]) -> String {
    match ir {
        SchemaIr::String(c) => {
            let mut s = "z.string()".to_string();
            if let Some(fmt) = &c.format {
                s.push_str(match fmt {
                    StringFormat::Email => ".email()",
                    StringFormat::Uri => ".url()",
                    StringFormat::Uuid => ".uuid()",
                    StringFormat::DateTime => ".datetime()",
                    StringFormat::Date => ".date()",
                    StringFormat::Time => ".time()",
                    StringFormat::Duration => ".duration()",
                    StringFormat::Ip => ".ip()",
                    StringFormat::Ipv4 => ".ip({ version: \"v4\" })",
                    StringFormat::Ipv6 => ".ip({ version: \"v6\" })",
                    StringFormat::Hostname => "",
                    StringFormat::Base64 => ".base64()",
                });
            }
            if let Some(min) = c.min_length {
                s.push_str(&format!(".min({min})"));
            }
            if let Some(max) = c.max_length {
                s.push_str(&format!(".max({max})"));
            }
            if let Some(pat) = &c.pattern {
                // Add unicode flag if pattern uses Unicode property escapes
                let flag = if pat.contains("\\p{") || pat.contains("\\P{") { "u" } else { "" };
                s.push_str(&format!(".regex(/{pat}/{flag})"));
            }
            s
        }

        SchemaIr::Number(c) => {
            let mut s = "z.number()".to_string();
            s.push_str(&number_constraints(c));
            s
        }

        SchemaIr::Integer(c) => {
            let mut s = "z.number().int()".to_string();
            s.push_str(&number_constraints(c));
            s
        }

        SchemaIr::Boolean => "z.boolean()".to_string(),
        SchemaIr::Null => "z.null()".to_string(),
        SchemaIr::Any => "z.any()".to_string(),
        SchemaIr::Never => "z.never()".to_string(),
        SchemaIr::Unknown => "z.unknown()".to_string(),

        SchemaIr::Literal(lit) => emit_literal(lit),

        SchemaIr::ConstEqual(val) => {
            let json = serde_json::to_string(val).unwrap_or_else(|_| "null".to_string());
            format!(
                "z.any().superRefine((val, ctx) => {{ if (!_deepEqual(val, {json})) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Must equal const value\" }}); }})"
            )
        }

        SchemaIr::Enum(cases) => {
            let items: Vec<String> = cases
                .iter()
                .map(|c| format!("\"{}\"", escape_string(c)))
                .collect();
            format!("z.enum([{}])", items.join(", "))
        }

        SchemaIr::MixedEnum(lits) => {
            let items: Vec<String> = lits
                .iter()
                .map(|lit| emit_literal(lit))
                .collect();
            format!("z.union([{}])", items.join(", "))
        }

        SchemaIr::ComplexEnum(values) => {
            let json = serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string());
            format!(
                "z.any().superRefine((val, ctx) => {{ const allowed = {json}; if (!allowed.some((a: unknown) => _deepEqual(val, a))) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Must be one of the allowed values\" }}); }})"
            )
        }

        SchemaIr::Array(arr) => {
            let mut s = format!("z.array({})", zod_for(&arr.items, defs));
            if let Some(min) = arr.min_items {
                s.push_str(&format!(".min({min})"));
            }
            if let Some(max) = arr.max_items {
                s.push_str(&format!(".max({max})"));
            }
            // uniqueItems
            if arr.unique_items {
                s.push_str(".superRefine((val, ctx) => { const seen = new Set(val.map((v: unknown) => _stableStringify(v))); if (seen.size !== val.length) ctx.addIssue({ code: z.ZodIssueCode.custom, message: \"Array items must be unique\" }); })");
            }
            // contains
            if let Some(contains) = &arr.contains {
                let contains_zod = zod_for(&contains.schema, defs);
                let min = contains.min_contains.unwrap_or(1);
                let mut checks = Vec::new();
                checks.push(format!(
                    "if (matchCount < {min}) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Array must contain at least {min} matching items\" }});"
                ));
                if let Some(max) = contains.max_contains {
                    checks.push(format!(
                        "if (matchCount > {max}) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Array must contain at most {max} matching items\" }});"
                    ));
                }
                s.push_str(&format!(
                    ".superRefine((val, ctx) => {{ const matchCount = val.filter((v: unknown) => {contains_zod}.safeParse(v).success).length; {} }})",
                    checks.join(" ")
                ));
            }
            s
        }

        SchemaIr::Tuple(tuple) => {
            // JSON Schema allows shorter arrays for prefixItems — z.tuple() doesn't.
            // Use z.array(z.any()).superRefine() for correct semantics.
            let prefix_count = tuple.items.len();
            let mut checks = Vec::new();

            for (i, item) in tuple.items.iter().enumerate() {
                let item_zod = zod_for(item, defs);
                checks.push(format!(
                    "if (arr.length > {i}) {{ const r = {item_zod}.safeParse(arr[{i}]); if (!r.success) r.error.issues.forEach(iss => ctx.addIssue({{...iss, path: [{i}, ...iss.path]}})); }}"
                ));
            }

            match &tuple.rest {
                None => {
                    // items: false — no additional items beyond prefix
                    checks.push(format!(
                        "if (arr.length > {prefix_count}) ctx.addIssue({{ code: z.ZodIssueCode.too_big, maximum: {prefix_count}, type: \"array\", inclusive: true, message: \"Array must have at most {prefix_count} items\" }});"
                    ));
                }
                Some(rest) => {
                    let rest_zod = zod_for(rest, defs);
                    if !matches!(rest.as_ref(), SchemaIr::Any) {
                        checks.push(format!(
                            "for (let i = {prefix_count}; i < arr.length; i++) {{ const r = {rest_zod}.safeParse(arr[i]); if (!r.success) r.error.issues.forEach(iss => ctx.addIssue({{...iss, path: [i, ...iss.path]}})); }}"
                        ));
                    }
                }
            }

            let mut s = format!(
                "z.array(z.any()).superRefine((val, ctx) => {{ const arr = val; {} }})",
                checks.join(" ")
            );

            // Apply min/max items
            if let Some(min) = tuple.min_items {
                s = format!("{s}.min({min})");
                // Note: Zod doesn't support chaining .min() after .superRefine() on z.array()
                // We need to add min check inside the superRefine instead
            }
            // Actually, add constraints via superRefine since z.array(z.any()) is what we use
            // Let me restructure: add min/max/unique/contains as additional checks
            // These are already in the superRefine
            if tuple.unique_items {
                s.push_str(".superRefine((val, ctx) => { const seen = new Set(val.map((v: unknown) => _stableStringify(v))); if (seen.size !== val.length) ctx.addIssue({ code: z.ZodIssueCode.custom, message: \"Array items must be unique\" }); })");
            }
            if let Some(min) = tuple.min_items {
                s.push_str(&format!(".superRefine((val, ctx) => {{ if (val.length < {min}) ctx.addIssue({{ code: z.ZodIssueCode.too_small, minimum: {min}, type: \"array\", inclusive: true, message: \"Array must have at least {min} items\" }}); }})"));
            }
            if let Some(max) = tuple.max_items {
                s.push_str(&format!(".superRefine((val, ctx) => {{ if (val.length > {max}) ctx.addIssue({{ code: z.ZodIssueCode.too_big, maximum: {max}, type: \"array\", inclusive: true, message: \"Array must have at most {max} items\" }}); }})"));
            }
            if let Some(contains) = &tuple.contains {
                let contains_zod = zod_for(&contains.schema, defs);
                let min = contains.min_contains.unwrap_or(1);
                let mut c_checks = Vec::new();
                c_checks.push(format!(
                    "if (matchCount < {min}) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Array must contain at least {min} matching items\" }});"
                ));
                if let Some(max) = contains.max_contains {
                    c_checks.push(format!(
                        "if (matchCount > {max}) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Array must contain at most {max} matching items\" }});"
                    ));
                }
                s.push_str(&format!(
                    ".superRefine((val, ctx) => {{ const matchCount = val.filter((v: unknown) => {contains_zod}.safeParse(v).success).length; {} }})",
                    c_checks.join(" ")
                ));
            }
            s
        }

        SchemaIr::Object(obj) => emit_object(obj, defs),

        SchemaIr::Record(rec) => {
            format!(
                "z.record({}, {})",
                zod_for(&rec.key, defs),
                zod_for(&rec.value, defs)
            )
        }

        SchemaIr::Union(members) => {
            if members.len() == 1 {
                return zod_for(&members[0], defs);
            }
            let parts: Vec<String> = members.iter().map(|m| zod_for(m, defs)).collect();
            format!("z.union([{}])", parts.join(", "))
        }

        SchemaIr::ExclusiveUnion(members) => {
            if members.len() == 1 {
                return zod_for(&members[0], defs);
            }
            let branch_schemas: Vec<String> =
                members.iter().map(|m| zod_for(m, defs)).collect();
            let branches_arr = branch_schemas.join(", ");
            format!(
                "z.any().superRefine((val, ctx) => {{ const branches = [{}]; const matches = branches.filter(s => s.safeParse(val).success).length; if (matches === 0) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Value must match exactly one of the oneOf schemas\" }}); if (matches > 1) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Value must match exactly one of the oneOf schemas, but matched multiple\" }}); }})",
                branches_arr
            )
        }

        SchemaIr::Intersection(members) => {
            if members.is_empty() {
                return "z.unknown()".to_string();
            }
            if members.len() == 1 {
                return zod_for(&members[0], defs);
            }
            // Fold left
            let mut result = format!(
                "z.intersection({}, {})",
                zod_for(&members[0], defs),
                zod_for(&members[1], defs)
            );
            for m in &members[2..] {
                result = format!("z.intersection({result}, {})", zod_for(m, defs));
            }
            result
        }

        SchemaIr::Nullable(inner) => format!("{}.nullable()", zod_for(inner, defs)),
        SchemaIr::Optional(inner) => format!("{}.optional()", zod_for(inner, defs)),

        SchemaIr::Default(inner, val) => {
            let val_str =
                serde_json::to_string(val).unwrap_or_else(|_| "undefined".to_string());
            format!("{}.default({})", zod_for(inner, defs), val_str)
        }

        SchemaIr::Describe(inner, desc) => {
            format!(
                "{}.describe(\"{}\")",
                zod_for(inner, defs),
                escape_string(desc)
            )
        }

        SchemaIr::Not { base, not_schema } => {
            let base_zod = zod_for(base, defs);
            let not_zod = zod_for(not_schema, defs);
            format!(
                "{base_zod}.superRefine((val, ctx) => {{ if ({not_zod}.safeParse(val).success) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Value must not match excluded schema\" }}); }})"
            )
        }

        SchemaIr::Conditional {
            base,
            if_schema,
            then_schema,
            else_schema,
        } => {
            let base_zod = zod_for(base, defs);
            let if_zod = zod_for(if_schema, defs);
            let mut body = format!("const ifResult = {if_zod}.safeParse(val); ");
            if let Some(then_s) = then_schema {
                let then_zod = zod_for(then_s, defs);
                body.push_str(&format!(
                    "if (ifResult.success) {{ const r = {then_zod}.safeParse(val); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }}"
                ));
            }
            if let Some(else_s) = else_schema {
                let else_zod = zod_for(else_s, defs);
                if then_schema.is_some() {
                    body.push_str(&format!(
                        " else {{ const r = {else_zod}.safeParse(val); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }}"
                    ));
                } else {
                    body.push_str(&format!(
                        "if (!ifResult.success) {{ const r = {else_zod}.safeParse(val); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }}"
                    ));
                }
            }
            format!("{base_zod}.superRefine((val, ctx) => {{ {body} }})")
        }

        SchemaIr::TypeGuarded(guards) => {
            let mut checks = Vec::new();
            for (guard, schema) in guards {
                let type_check = match guard {
                    TypeGuard::String => "typeof val === \"string\"",
                    TypeGuard::Number => "typeof val === \"number\"",
                    TypeGuard::Object => "val !== null && typeof val === \"object\" && !Array.isArray(val)",
                    TypeGuard::Array => "Array.isArray(val)",
                };
                let schema_zod = zod_for(schema, defs);
                checks.push(format!(
                    "if ({type_check}) {{ const r = {schema_zod}.safeParse(val); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }}"
                ));
            }
            format!(
                "z.any().superRefine((val, ctx) => {{ {} }})",
                checks.join(" ")
            )
        }

        SchemaIr::Ref(name) => format!("{name}Schema"),
        SchemaIr::Lazy(name) => format!("z.lazy(() => {name}Schema)"),
    }
}

fn emit_literal(lit: &LiteralValue) -> String {
    match lit {
        LiteralValue::String(s) => format!("z.literal(\"{}\")", escape_string(s)),
        LiteralValue::Number(n) => format!("z.literal({n})"),
        LiteralValue::Integer(i) => format!("z.literal({i})"),
        LiteralValue::Bool(b) => format!("z.literal({b})"),
        LiteralValue::Null => "z.null()".to_string(),
    }
}

fn emit_object(obj: &ObjectSchema, defs: &[DefEntry]) -> String {
    let fields: Vec<String> = obj
        .fields
        .iter()
        .map(|(k, f)| {
            let schema_str = zod_for(&f.schema, defs);
            let field_str = if f.required {
                schema_str
            } else {
                format!("{schema_str}.optional()")
            };
            let key = if needs_quoting(k) {
                format!("\"{}\"", escape_string(k))
            } else {
                k.clone()
            };
            format!("  {key}: {field_str}")
        })
        .collect();

    let base = if fields.is_empty() {
        "z.object({})".to_string()
    } else {
        format!("z.object({{\n{}\n}})", fields.join(",\n"))
    };

    // Determine if we need .passthrough() — superRefine-based constraints need to see all keys
    let needs_passthrough = !obj.pattern_properties.is_empty()
        || obj.property_names.is_some()
        || obj.min_properties.is_some()
        || obj.max_properties.is_some()
        || !obj.dependent_required.is_empty()
        || !obj.dependent_schemas.is_empty();

    // additionalProperties handling
    // When patternProperties or additionalProperties schema is present, use superRefine
    // (because .strict() and .and(z.record(...)) don't handle the interaction correctly)
    let mut s = match &obj.additional_properties {
        AdditionalProperties::Forbidden
            if obj.pattern_properties.is_empty() && !needs_passthrough =>
        {
            format!("{base}.strict()")
        }
        _ if needs_passthrough
            || matches!(obj.additional_properties, AdditionalProperties::Schema(_)) =>
        {
            format!("{base}.passthrough()")
        }
        _ => base,
    };

    // additionalProperties as schema — validate extra properties via superRefine
    if let AdditionalProperties::Schema(ap) = &obj.additional_properties {
        if obj.pattern_properties.is_empty() {
            let ap_zod = zod_for(ap, defs);
            let named_keys: Vec<String> = obj
                .fields
                .keys()
                .map(|k| format!("\"{}\"", escape_string(k)))
                .collect();
            let named_set = if named_keys.is_empty() {
                "new Set<string>()".to_string()
            } else {
                format!("new Set([{}])", named_keys.join(", "))
            };
            s.push_str(&format!(
                ".superRefine((val, ctx) => {{ const named = {named_set}; for (const [key, value] of Object.entries(val as Record<string, unknown>)) {{ if (!named.has(key)) {{ const r = {ap_zod}.safeParse(value); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }} }} }})"
            ));
        }
    }
    // additionalProperties: false with passthrough (when other superRefine constraints exist)
    if matches!(obj.additional_properties, AdditionalProperties::Forbidden) && needs_passthrough {
        if obj.pattern_properties.is_empty() {
            let named_keys: Vec<String> = obj
                .fields
                .keys()
                .map(|k| format!("\"{}\"", escape_string(k)))
                .collect();
            let named_set = if named_keys.is_empty() {
                "new Set<string>()".to_string()
            } else {
                format!("new Set([{}])", named_keys.join(", "))
            };
            s.push_str(&format!(
                ".superRefine((val, ctx) => {{ const named = {named_set}; for (const key of Object.keys(val as Record<string, unknown>)) {{ if (!named.has(key)) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: `Unexpected property: ${{key}}` }}); }} }})"
            ));
        }
    }

    // patternProperties + additionalProperties interaction
    if !obj.pattern_properties.is_empty() {
        let named_keys: Vec<String> = obj.fields.keys().map(|k| format!("\"{}\"", escape_string(k))).collect();
        let named_set = if named_keys.is_empty() {
            "new Set<string>()".to_string()
        } else {
            format!("new Set([{}])", named_keys.join(", "))
        };

        let mut pp_checks = Vec::new();
        for (pattern, schema) in &obj.pattern_properties {
            let schema_zod = zod_for(schema, defs);
            pp_checks.push(format!(
                "if (/{pattern}/.test(key)) {{ matched = true; const r = {schema_zod}.safeParse(value); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }}"
            ));
        }

        let reject_extra = matches!(obj.additional_properties, AdditionalProperties::Forbidden);

        // Build the full superRefine for patternProperties
        // patternProperties applies to ALL keys (including named properties)
        // but additionalProperties only applies to keys NOT in properties AND NOT matched by patternProperties
        let refine_body = if let AdditionalProperties::Schema(ap) = &obj.additional_properties {
            let ap_zod = zod_for(ap, defs);
            format!(
                "const named = {named_set}; for (const [key, value] of Object.entries(val as Record<string, unknown>)) {{ let matched = false; {} if (!named.has(key) && !matched) {{ const r = {ap_zod}.safeParse(value); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }} }}",
                pp_checks.join(" ")
            )
        } else {
            format!(
                "const named = {named_set}; for (const [key, value] of Object.entries(val as Record<string, unknown>)) {{ let matched = false; {}{} }}",
                pp_checks.join(" "),
                if reject_extra { " if (!named.has(key) && !matched) ctx.addIssue({ code: z.ZodIssueCode.custom, message: `Unexpected property: ${key}` });" } else { "" }
            )
        };

        s.push_str(&format!(
            ".superRefine((val, ctx) => {{ {refine_body} }})"
        ));
    }

    // propertyNames
    if let Some(pn) = &obj.property_names {
        let key_schema = zod_for(pn, defs);
        s.push_str(&format!(
            ".superRefine((val, ctx) => {{ for (const key of Object.keys(val as Record<string, unknown>)) {{ const r = {key_schema}.safeParse(key); if (!r.success) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: `Invalid property name: ${{key}}` }}); }} }})"
        ));
    }

    // minProperties / maxProperties
    if obj.min_properties.is_some() || obj.max_properties.is_some() {
        let mut checks = Vec::new();
        if let Some(min) = obj.min_properties {
            checks.push(format!(
                "if (count < {min}) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Object must have at least {min} properties\" }});"
            ));
        }
        if let Some(max) = obj.max_properties {
            checks.push(format!(
                "if (count > {max}) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Object must have at most {max} properties\" }});"
            ));
        }
        s.push_str(&format!(
            ".superRefine((val, ctx) => {{ const count = Object.keys(val as Record<string, unknown>).length; {} }})",
            checks.join(" ")
        ));
    }

    // Explicit required-field check for fields with z.any() schema
    // (z.any() accepts undefined, so z.object won't enforce presence on its own)
    let any_required: Vec<&String> = obj
        .fields
        .iter()
        .filter(|(_, f)| f.required && matches!(f.schema, SchemaIr::Any))
        .map(|(k, _)| k)
        .collect();
    if !any_required.is_empty() {
        let req_keys: Vec<String> = any_required
            .iter()
            .map(|k| format!("\"{}\"", escape_string(k)))
            .collect();
        s.push_str(&format!(
            ".superRefine((val, ctx) => {{ const obj = val as Record<string, unknown>; for (const key of [{}]) {{ if (!Object.hasOwn(obj, key)) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: `Missing required property: ${{key}}` }}); }} }})",
            req_keys.join(", ")
        ));
    }

    // dependentRequired
    for (prop, required) in &obj.dependent_required {
        let prop_escaped = escape_string(prop);
        let req_checks: Vec<String> = required
            .iter()
            .map(|r| {
                let r_escaped = escape_string(r);
                format!(
                    "if (!Object.hasOwn(obj, \"{r_escaped}\")) ctx.addIssue({{ code: z.ZodIssueCode.custom, message: \"Property '{r_escaped}' is required when '{prop_escaped}' is present\" }});"
                )
            })
            .collect();
        s.push_str(&format!(
            ".superRefine((val, ctx) => {{ const obj = val as Record<string, unknown>; if (Object.hasOwn(obj, \"{prop_escaped}\")) {{ {} }} }})",
            req_checks.join(" ")
        ));
    }

    // dependentSchemas
    for (prop, schema) in &obj.dependent_schemas {
        let prop_escaped = escape_string(prop);
        let schema_zod = zod_for(schema, defs);
        s.push_str(&format!(
            ".superRefine((val, ctx) => {{ if (Object.hasOwn(val as Record<string, unknown>, \"{prop_escaped}\")) {{ const r = {schema_zod}.safeParse(val); if (!r.success) r.error.issues.forEach(i => ctx.addIssue(i)); }} }})"
        ));
    }

    s
}

/// Generate a TypeScript type string for recursive schemas (for z.ZodType<T> annotations)
fn zod_ts_type(ir: &SchemaIr, defs: &[DefEntry]) -> String {
    match ir {
        SchemaIr::String(_) => "string".to_string(),
        SchemaIr::Number(_) | SchemaIr::Integer(_) => "number".to_string(),
        SchemaIr::Boolean => "boolean".to_string(),
        SchemaIr::Null => "null".to_string(),
        SchemaIr::Any => "any".to_string(),
        SchemaIr::Never => "never".to_string(),
        SchemaIr::Unknown => "unknown".to_string(),
        SchemaIr::Literal(LiteralValue::String(s)) => format!("\"{}\"", escape_string(s)),
        SchemaIr::Literal(LiteralValue::Number(n)) => format!("{n}"),
        SchemaIr::Literal(LiteralValue::Integer(i)) => format!("{i}"),
        SchemaIr::Literal(LiteralValue::Bool(b)) => format!("{b}"),
        SchemaIr::Literal(LiteralValue::Null) => "null".to_string(),
        SchemaIr::ConstEqual(_) | SchemaIr::ComplexEnum(_) => "any".to_string(),
        SchemaIr::Array(arr) => format!("{}[]", zod_ts_type(&arr.items, defs)),
        SchemaIr::Tuple(_) => "any[]".to_string(),
        SchemaIr::Object(obj) => {
            let fields: Vec<String> = obj
                .fields
                .iter()
                .map(|(k, f)| {
                    let opt = if f.required { "" } else { "?" };
                    format!("{k}{opt}: {}", zod_ts_type(&f.schema, defs))
                })
                .collect();
            format!("{{ {} }}", fields.join("; "))
        }
        SchemaIr::Record(rec) => {
            format!("Record<string, {}>", zod_ts_type(&rec.value, defs))
        }
        SchemaIr::Union(members) | SchemaIr::ExclusiveUnion(members) => {
            let parts: Vec<String> = members.iter().map(|m| zod_ts_type(m, defs)).collect();
            parts.join(" | ")
        }
        SchemaIr::Intersection(members) => {
            let parts: Vec<String> = members.iter().map(|m| zod_ts_type(m, defs)).collect();
            parts.join(" & ")
        }
        SchemaIr::Nullable(inner) => format!("{} | null", zod_ts_type(inner, defs)),
        SchemaIr::Optional(inner) => zod_ts_type(inner, defs),
        SchemaIr::Default(inner, _) => zod_ts_type(inner, defs),
        SchemaIr::Describe(inner, _) => zod_ts_type(inner, defs),
        SchemaIr::Not { base, .. } => zod_ts_type(base, defs),
        SchemaIr::Conditional { base, .. } => zod_ts_type(base, defs),
        SchemaIr::TypeGuarded(_) => "any".to_string(),
        SchemaIr::Ref(name) | SchemaIr::Lazy(name) => name.clone(),
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
                .map(|lit| zod_ts_type(&SchemaIr::Literal(lit.clone()), defs))
                .collect();
            parts.join(" | ")
        }
    }
}

fn number_constraints(c: &NumberConstraints) -> String {
    let mut s = String::new();
    if let Some(gt) = c.exclusive_minimum {
        s.push_str(&format!(".gt({gt})"));
    } else if let Some(gte) = c.minimum {
        s.push_str(&format!(".gte({gte})"));
    }
    if let Some(lt) = c.exclusive_maximum {
        s.push_str(&format!(".lt({lt})"));
    } else if let Some(lte) = c.maximum {
        s.push_str(&format!(".lte({lte})"));
    }
    if let Some(mul) = c.multiple_of {
        s.push_str(&format!(".multipleOf({mul})"));
    }
    s
}

