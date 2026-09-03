use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use serde_json::Value;

use crate::ir::*;

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("unsupported JSON Schema feature: {0}")]
    Unsupported(String),
}

pub struct Converter {
    defs: HashMap<String, Value>,
    visiting: HashSet<String>,
    pub recursive_refs: HashSet<String>,
    converted_defs: HashMap<String, SchemaIr>,
    root_schema: Value,
    /// Maps absolute URI → sub-schema (built from $id entries)
    id_registry: HashMap<String, Value>,
    /// Current base URI for resolving relative $ref values
    base_uri: Option<String>,
}

impl Converter {
    pub fn convert(root: &Value) -> Result<ConvertedSchema, ConvertError> {
        // Extract root $id as the initial base URI
        let root_id = root
            .as_object()
            .and_then(|o| o.get("$id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut c = Converter {
            defs: HashMap::new(),
            visiting: HashSet::new(),
            recursive_refs: HashSet::new(),
            converted_defs: HashMap::new(),
            root_schema: root.clone(),
            id_registry: HashMap::new(),
            base_uri: root_id.clone(),
        };

        // Pre-pass: build $id registry by walking the entire schema tree
        if let Some(base) = &root_id {
            c.id_registry.insert(base.clone(), root.clone());
        }
        build_id_registry(root, &c.base_uri, &mut c.id_registry);

        // Extract $defs / definitions
        if let Some(obj) = root.as_object() {
            for key in &["$defs", "definitions"] {
                if let Some(Value::Object(map)) = obj.get(*key) {
                    for (k, v) in map {
                        c.defs.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        // Add root $id to visiting set for cycle detection
        if let Some(base) = &root_id {
            c.visiting.insert(base.clone());
        }

        let mut root_ir = c.convert_schema(root)?;

        // Remove root from visiting
        if let Some(base) = &root_id {
            c.visiting.remove(base);

            // If the root was referenced by its $id (cycle detected),
            // register it as a def so other schemas can reference it
            let root_def_name = uri_to_def_name(base);
            if c.recursive_refs.contains(&root_def_name) {
                let pascal = crate::util::to_pascal_case(&root_def_name);
                c.converted_defs.insert(root_def_name.clone(), root_ir.clone());
                c.defs
                    .entry(root_def_name)
                    .or_insert_with(|| root.clone());
                // Replace root IR with a Ref to the def
                root_ir = SchemaIr::Ref(pascal);
            }
        }

        // Convert all referenced defs (collect after root-as-def registration)
        let def_names: Vec<String> = c.defs.keys().cloned().collect();
        for name in &def_names {
            if !c.converted_defs.contains_key(name) {
                let def_schema = c.defs[name].clone();
                let ir = c.convert_schema(&def_schema)?;
                c.converted_defs.insert(name.clone(), ir);
            }
        }

        // Build ordered defs using topological sort so that dependencies are
        // emitted before the defs that reference them.
        let sorted_names = topological_sort_defs(&def_names, &c.converted_defs);
        let mut defs = Vec::new();
        for name in &sorted_names {
            if let Some(ir) = c.converted_defs.get(name) {
                let is_recursive = c.recursive_refs.contains(name);
                defs.push(DefEntry {
                    name: crate::util::to_pascal_case(name),
                    schema: ir.clone(),
                    is_recursive,
                });
            }
        }

        Ok(ConvertedSchema {
            defs,
            root: root_ir,
        })
    }

    fn convert_schema(&mut self, schema: &Value) -> Result<SchemaIr, ConvertError> {
        // Handle boolean schemas
        match schema {
            Value::Bool(true) => return Ok(SchemaIr::Any),
            Value::Bool(false) => return Ok(SchemaIr::Never),
            _ => {}
        }

        let obj = match schema.as_object() {
            Some(o) => o,
            None => return Ok(SchemaIr::Unknown),
        };

        // Handle $ref — may have sibling keywords in Draft 2020-12
        if let Some(Value::String(ref_str)) = obj.get("$ref") {
            let ref_ir = self.resolve_ref(ref_str)?;

            // Check for sibling keywords alongside $ref (Draft 2020-12 behavior)
            let has_siblings = obj.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "$ref" | "$schema" | "$defs" | "definitions" | "$id"
                        | "$anchor" | "$comment" | "title" | "description" | "default"
                )
            });

            if !has_siblings {
                return Ok(ref_ir);
            }

            // Build IR from sibling keywords and intersect with ref
            let mut sibling_obj = obj.clone();
            sibling_obj.remove("$ref");
            let sibling_ir = self.convert_schema(&Value::Object(sibling_obj))?;
            return Ok(SchemaIr::Intersection(vec![ref_ir, sibling_ir]));
        }

        // Collect metadata
        let title = obj.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let default_val = obj.get("default").cloned();
        let describe = match (title, description) {
            (Some(t), Some(d)) => Some(format!("{t}: {d}")),
            (Some(t), None) => Some(t),
            (None, d) => d,
        };

        // Build base IR from const/enum/composition/type keywords
        let base_ir = self.build_base_ir(obj)?;

        // Apply "not" refinement
        let ir = if let Some(not_val) = obj.get("not") {
            let not_ir = self.convert_schema(not_val)?;
            SchemaIr::Not {
                base: Box::new(base_ir),
                not_schema: Box::new(not_ir),
            }
        } else {
            base_ir
        };

        // Apply if/then/else refinement
        let ir = if let Some(if_val) = obj.get("if") {
            let if_ir = self.convert_schema(if_val)?;
            let then_ir = obj
                .get("then")
                .map(|v| self.convert_schema(v))
                .transpose()?;
            let else_ir = obj
                .get("else")
                .map(|v| self.convert_schema(v))
                .transpose()?;
            if then_ir.is_some() || else_ir.is_some() {
                SchemaIr::Conditional {
                    base: Box::new(ir),
                    if_schema: Box::new(if_ir),
                    then_schema: then_ir.map(Box::new),
                    else_schema: else_ir.map(Box::new),
                }
            } else {
                ir
            }
        } else {
            ir
        };

        // Apply metadata modifiers (description, default)
        Ok(apply_modifiers(ir, describe, default_val))
    }

    /// Build the base IR from const/enum/composition/type keywords (no modifiers).
    fn build_base_ir(
        &mut self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<SchemaIr, ConvertError> {
        // Handle const
        if let Some(const_val) = obj.get("const") {
            return Ok(const_from_value(const_val));
        }

        // Handle enum
        if let Some(Value::Array(cases)) = obj.get("enum") {
            return Ok(enum_from_values(cases));
        }

        // Collect composition and type results separately, then combine
        let composition_ir = self.build_composition_ir(obj)?;
        let type_ir = self.build_type_ir(obj)?;

        match (type_ir, composition_ir) {
            (None, None) => Ok(SchemaIr::Any),
            (Some(t), None) => Ok(t),
            (None, Some(c)) => Ok(c),
            (Some(t), Some(c)) => Ok(SchemaIr::Intersection(vec![t, c])),
        }
    }

    /// Build IR from composition keywords (allOf, anyOf, oneOf).
    /// If multiple composition keywords are present, they're intersected (all must pass).
    fn build_composition_ir(
        &mut self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Option<SchemaIr>, ConvertError> {
        let mut results = Vec::new();

        // Handle allOf
        if let Some(Value::Array(members)) = obj.get("allOf") {
            results.push(self.handle_all_of(members)?);
        }

        // Handle anyOf
        if let Some(Value::Array(members)) = obj.get("anyOf") {
            let (nullable, non_null) = extract_nullable(members);
            let ir = if non_null.is_empty() {
                SchemaIr::Null
            } else if non_null.len() == 1 {
                self.convert_schema(&non_null[0])?
            } else {
                let parts = non_null
                    .iter()
                    .map(|s| self.convert_schema(s))
                    .collect::<Result<Vec<_>, _>>()?;
                SchemaIr::Union(parts)
            };
            results.push(if nullable {
                SchemaIr::Nullable(Box::new(ir))
            } else {
                ir
            });
        }

        // Handle oneOf (exclusive — exactly one branch must match)
        if let Some(Value::Array(members)) = obj.get("oneOf") {
            let (nullable, non_null) = extract_nullable(members);
            let ir = if non_null.is_empty() {
                SchemaIr::Null
            } else if non_null.len() == 1 {
                self.convert_schema(&non_null[0])?
            } else {
                let parts = non_null
                    .iter()
                    .map(|s| self.convert_schema(s))
                    .collect::<Result<Vec<_>, _>>()?;
                SchemaIr::ExclusiveUnion(parts)
            };
            results.push(if nullable {
                SchemaIr::Nullable(Box::new(ir))
            } else {
                ir
            });
        }

        match results.len() {
            0 => Ok(None),
            1 => Ok(Some(results.into_iter().next().unwrap())),
            _ => Ok(Some(SchemaIr::Intersection(results))),
        }
    }

    /// Build IR from type and type-specific keywords.
    fn build_type_ir(
        &mut self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Option<SchemaIr>, ConvertError> {
        let type_val = obj.get("type");

        // Handle type array (e.g. ["string", "null"])
        if let Some(Value::Array(types)) = type_val {
            let has_null = types.iter().any(|t| t.as_str() == Some("null"));
            let non_null_types: Vec<&Value> =
                types.iter().filter(|t| t.as_str() != Some("null")).collect();

            if non_null_types.is_empty() {
                return Ok(Some(SchemaIr::Null));
            }

            let inner = if non_null_types.len() == 1 {
                let single_type =
                    Value::String(non_null_types[0].as_str().unwrap_or("").to_string());
                let mut fake_obj = obj.clone();
                fake_obj.insert("type".to_string(), single_type);
                self.convert_typed_schema(&fake_obj)?
            } else {
                let parts = non_null_types
                    .iter()
                    .map(|t| {
                        let mut fake_obj = obj.clone();
                        fake_obj.insert(
                            "type".to_string(),
                            Value::String(t.as_str().unwrap_or("").to_string()),
                        );
                        self.convert_typed_schema(&fake_obj)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                SchemaIr::Union(parts)
            };

            return Ok(Some(if has_null {
                SchemaIr::Nullable(Box::new(inner))
            } else {
                inner
            }));
        }

        // Handle single type string
        if let Some(Value::String(type_str)) = type_val {
            return Ok(Some(self.convert_typed_schema_str(type_str, obj)?));
        }

        // No type — check for type-specific keywords
        self.build_typeless_ir(obj)
    }

    /// Handle schemas with no explicit type but with type-specific keywords.
    fn build_typeless_ir(
        &mut self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Option<SchemaIr>, ConvertError> {
        let has_string_kw = obj.contains_key("minLength")
            || obj.contains_key("maxLength")
            || obj.contains_key("pattern")
            || obj.contains_key("format");
        let has_number_kw = obj.contains_key("minimum")
            || obj.contains_key("maximum")
            || obj.contains_key("exclusiveMinimum")
            || obj.contains_key("exclusiveMaximum")
            || obj.contains_key("multipleOf");
        let has_object_kw = obj.contains_key("properties")
            || obj.contains_key("additionalProperties")
            || obj.contains_key("patternProperties")
            || obj.contains_key("required")
            || obj.contains_key("propertyNames")
            || obj.contains_key("minProperties")
            || obj.contains_key("maxProperties")
            || obj.contains_key("dependentRequired")
            || obj.contains_key("dependentSchemas");
        let has_array_kw = obj.contains_key("items")
            || obj.contains_key("prefixItems")
            || obj.contains_key("minItems")
            || obj.contains_key("maxItems")
            || obj.contains_key("uniqueItems")
            || obj.contains_key("contains");

        let any_kw = has_string_kw || has_number_kw || has_object_kw || has_array_kw;
        if !any_kw {
            return Ok(None);
        }

        let mut guards: Vec<(TypeGuard, Box<SchemaIr>)> = Vec::new();

        if has_string_kw {
            guards.push((
                TypeGuard::String,
                Box::new(SchemaIr::String(parse_string_constraints(obj))),
            ));
        }
        if has_number_kw {
            guards.push((
                TypeGuard::Number,
                Box::new(SchemaIr::Number(parse_number_constraints(obj))),
            ));
        }
        if has_object_kw {
            guards.push((TypeGuard::Object, Box::new(self.handle_object(obj)?)));
        }
        if has_array_kw {
            guards.push((TypeGuard::Array, Box::new(self.handle_array(obj)?)));
        }

        Ok(Some(SchemaIr::TypeGuarded(guards)))
    }

    /// Convert a schema with a known type string.
    fn convert_typed_schema_str(
        &mut self,
        type_str: &str,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<SchemaIr, ConvertError> {
        match type_str {
            "string" => Ok(SchemaIr::String(parse_string_constraints(obj))),
            "number" => Ok(SchemaIr::Number(parse_number_constraints(obj))),
            "integer" => Ok(SchemaIr::Integer(parse_number_constraints(obj))),
            "boolean" => Ok(SchemaIr::Boolean),
            "null" => Ok(SchemaIr::Null),
            "array" => self.handle_array(obj),
            "object" => self.handle_object(obj),
            _ => Ok(SchemaIr::Unknown),
        }
    }

    /// Convert a schema with a type in the "type" field (backward compat helper).
    fn convert_typed_schema(
        &mut self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<SchemaIr, ConvertError> {
        let type_str = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if type_str.is_empty() {
            // No type — check for typeless keywords
            Ok(self.build_typeless_ir(obj)?.unwrap_or(SchemaIr::Any))
        } else {
            self.convert_typed_schema_str(type_str, obj)
        }
    }

    fn resolve_ref(&mut self, ref_str: &str) -> Result<SchemaIr, ConvertError> {
        // Try resolving via $id registry first (for non-fragment refs)
        if !ref_str.starts_with('#') {
            // Resolve relative URI against current base
            let absolute = if is_absolute_uri(ref_str) {
                ref_str.to_string()
            } else if let Some(base) = &self.base_uri {
                resolve_relative_uri(base, ref_str)
            } else {
                ref_str.to_string()
            };

            // Split off fragment if present (e.g., "urn:uuid:...#/$defs/bar")
            let (uri_part, fragment) = if let Some(idx) = absolute.find('#') {
                (&absolute[..idx], Some(&absolute[idx..]))
            } else {
                (absolute.as_str(), None)
            };

            if let Some(sub_schema) = self.id_registry.get(uri_part).cloned() {
                // If there's a fragment, resolve it within the sub-schema
                if let Some(frag) = fragment {
                    if let Some(pointer) = frag.strip_prefix('#') {
                        if pointer.is_empty() {
                            // Just "#" — fall through to convert the whole sub-schema
                        } else if let Some(found) = resolve_json_pointer(&sub_schema, pointer) {
                            return self.convert_schema(&found);
                        } else {
                            return Ok(SchemaIr::Unknown);
                        }
                    }
                }

                // Derive a stable name from the URI for use as a def
                let def_name = uri_to_def_name(uri_part);
                let pascal = crate::util::to_pascal_case(&def_name);

                // Check for cycles
                let key = uri_part.to_string();
                if self.visiting.contains(&key) {
                    self.recursive_refs.insert(def_name.clone());
                    return Ok(SchemaIr::Lazy(pascal));
                }

                // Already converted?
                if self.converted_defs.contains_key(&def_name) {
                    return Ok(SchemaIr::Ref(pascal));
                }

                // Convert the sub-schema with its $id as the new base URI
                let old_base = self.base_uri.clone();
                self.base_uri = Some(uri_part.to_string());
                self.visiting.insert(key.clone());
                let ir = self.convert_schema(&sub_schema)?;
                self.visiting.remove(&key);
                self.base_uri = old_base;

                // Register as a def so it gets emitted in the module
                self.converted_defs.insert(def_name.clone(), ir);
                self.defs.entry(def_name.clone()).or_insert(sub_schema);
                return Ok(SchemaIr::Ref(pascal));
            }
        }

        // Handle root self-reference "#"
        if ref_str == "#" {
            let root_name = "Root".to_string();
            if self.visiting.contains(&root_name) {
                self.recursive_refs.insert(root_name.clone());
                return Ok(SchemaIr::Lazy(root_name));
            }
            // Convert root schema inline if not already done
            if let Some(ir) = self.converted_defs.get(&root_name).cloned() {
                return Ok(ir);
            }
            self.visiting.insert(root_name.clone());
            let root = self.root_schema.clone();
            let ir = self.convert_schema(&root)?;
            self.visiting.remove(&root_name);
            return Ok(ir);
        }

        // Handle fragment refs (#/...)
        // Resolve against the current $id scope, not just the root
        if let Some(pointer) = ref_str.strip_prefix('#') {
            if !pointer.is_empty() {
                // If we're in a non-root $id scope, resolve the fragment against that scope
                let in_non_root_scope = self
                    .base_uri
                    .as_ref()
                    .map(|b| {
                        self.root_schema
                            .as_object()
                            .and_then(|o| o.get("$id"))
                            .and_then(|v| v.as_str())
                            != Some(b.as_str())
                    })
                    .unwrap_or(false);

                if in_non_root_scope {
                    // Resolve against the current $id scope
                    if let Some(base) = &self.base_uri {
                        if let Some(scope_schema) = self.id_registry.get(base).cloned() {
                            if let Some(sub_schema) =
                                resolve_json_pointer(&scope_schema, pointer)
                            {
                                return self.convert_schema(&sub_schema);
                            }
                        }
                    }
                }

                // For root scope or fallback, check non-$defs pointers
                if !pointer.starts_with("/$defs/") && !pointer.starts_with("/definitions/") {
                    let root = self.root_schema.clone();
                    if let Some(sub_schema) = resolve_json_pointer(&root, pointer) {
                        return self.convert_schema(&sub_schema);
                    }
                }
            }
        }

        let name = extract_ref_name(ref_str);
        if name.is_empty() {
            return Ok(SchemaIr::Unknown);
        }

        if self.visiting.contains(&name) {
            self.recursive_refs.insert(name.clone());
            return Ok(SchemaIr::Lazy(crate::util::to_pascal_case(&name)));
        }

        if self.converted_defs.contains_key(&name) {
            return Ok(SchemaIr::Ref(crate::util::to_pascal_case(&name)));
        }

        if let Some(def_schema) = self.defs.get(&name).cloned() {
            self.visiting.insert(name.clone());
            let ir = self.convert_schema(&def_schema)?;
            self.visiting.remove(&name);
            self.converted_defs.insert(name.clone(), ir);
            Ok(SchemaIr::Ref(crate::util::to_pascal_case(&name)))
        } else {
            Ok(SchemaIr::Unknown)
        }
    }

    fn handle_all_of(&mut self, members: &[Value]) -> Result<SchemaIr, ConvertError> {
        // Check if all members are object schemas — if so, merge
        let all_objects = members.iter().all(is_object_schema);

        if all_objects {
            let mut merged_fields: IndexMap<String, FieldSchema> = IndexMap::new();
            let mut merged_required: HashSet<String> = HashSet::new();
            let mut merged_ap = AdditionalProperties::Allowed;

            for member in members {
                let obj = match member.as_object() {
                    Some(o) => o,
                    None => continue,
                };

                if let Some(Value::Array(req)) = obj.get("required") {
                    for r in req {
                        if let Some(s) = r.as_str() {
                            merged_required.insert(s.to_string());
                        }
                    }
                }

                if let Some(Value::Object(props)) = obj.get("properties") {
                    for (k, v) in props {
                        let field_ir = self.convert_schema(v)?;
                        merged_fields.insert(
                            k.clone(),
                            FieldSchema {
                                schema: field_ir,
                                required: merged_required.contains(k),
                            },
                        );
                    }
                }

                match obj.get("additionalProperties") {
                    Some(Value::Bool(false)) => merged_ap = AdditionalProperties::Forbidden,
                    Some(Value::Bool(true)) | None => {}
                    Some(ap_schema) => {
                        let ap_ir = self.convert_schema(ap_schema)?;
                        merged_ap = AdditionalProperties::Schema(Box::new(ap_ir));
                    }
                }
            }

            // Fix required flags after collecting all fields
            for (k, field) in &mut merged_fields {
                field.required = merged_required.contains(k);
            }

            Ok(SchemaIr::Object(ObjectSchema {
                fields: merged_fields,
                additional_properties: merged_ap,
                pattern_properties: Vec::new(),
                property_names: None,
                min_properties: None,
                max_properties: None,
                dependent_required: Vec::new(),
                dependent_schemas: Vec::new(),
            }))
        } else {
            let parts = members
                .iter()
                .map(|s| self.convert_schema(s))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SchemaIr::Intersection(parts))
        }
    }

    fn handle_array(
        &mut self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<SchemaIr, ConvertError> {
        let min_items = obj
            .get("minItems")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)));
        let max_items = obj
            .get("maxItems")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)));
        let unique_items = obj
            .get("uniqueItems")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // contains / minContains / maxContains
        let contains = if let Some(contains_schema) = obj.get("contains") {
            let schema = self.convert_schema(contains_schema)?;
            let min_contains = obj
                .get("minContains")
                .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)));
            let max_contains = obj
                .get("maxContains")
                .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)));
            Some(ContainsSchema {
                schema: Box::new(schema),
                min_contains,
                max_contains,
            })
        } else {
            None
        };

        // prefixItems (Draft 2020-12 tuple)
        if let Some(Value::Array(prefix)) = obj.get("prefixItems") {
            let items = prefix
                .iter()
                .map(|s| self.convert_schema(s))
                .collect::<Result<Vec<_>, _>>()?;
            // In Draft 2020-12, "items" alongside prefixItems is the rest schema
            let rest = if let Some(items_schema) = obj.get("items") {
                match items_schema {
                    Value::Bool(false) => None,
                    Value::Bool(true) => Some(Box::new(SchemaIr::Any)),
                    _ => Some(Box::new(self.convert_schema(items_schema)?)),
                }
            } else {
                Some(Box::new(SchemaIr::Any))
            };
            return Ok(SchemaIr::Tuple(TupleSchema {
                items,
                rest,
                unique_items,
                min_items,
                max_items,
                contains,
            }));
        }

        // items as array (Draft 4/7 tuple)
        if let Some(Value::Array(items_arr)) = obj.get("items") {
            let items = items_arr
                .iter()
                .map(|s| self.convert_schema(s))
                .collect::<Result<Vec<_>, _>>()?;
            let rest = match obj.get("additionalItems") {
                Some(Value::Bool(false)) => None,
                Some(Value::Bool(true)) | None => Some(Box::new(SchemaIr::Any)),
                Some(schema) => Some(Box::new(self.convert_schema(schema)?)),
            };
            return Ok(SchemaIr::Tuple(TupleSchema {
                items,
                rest,
                unique_items,
                min_items,
                max_items,
                contains,
            }));
        }

        // items as schema (or absent)
        let items_ir = if let Some(items_schema) = obj.get("items") {
            match items_schema {
                Value::Bool(true) => SchemaIr::Any,
                Value::Bool(false) => SchemaIr::Never,
                _ => self.convert_schema(items_schema)?,
            }
        } else {
            SchemaIr::Any
        };

        Ok(SchemaIr::Array(ArraySchema {
            items: Box::new(items_ir),
            min_items,
            max_items,
            unique_items,
            contains,
        }))
    }

    fn handle_object(
        &mut self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<SchemaIr, ConvertError> {
        let required_set: HashSet<String> = obj
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let has_properties = obj.contains_key("properties");
        let has_pattern_props = obj.contains_key("patternProperties");
        let ap_value = obj.get("additionalProperties");

        let additional_properties = match ap_value {
            Some(Value::Bool(false)) => AdditionalProperties::Forbidden,
            Some(Value::Bool(true)) | None => AdditionalProperties::Allowed,
            Some(ap_schema) => {
                let ap_ir = self.convert_schema(ap_schema)?;
                AdditionalProperties::Schema(Box::new(ap_ir))
            }
        };

        // patternProperties
        let pattern_properties = if let Some(Value::Object(pp)) = obj.get("patternProperties") {
            let mut result = Vec::new();
            for (pattern, schema) in pp {
                let ir = self.convert_schema(schema)?;
                result.push((pattern.clone(), ir));
            }
            result
        } else {
            Vec::new()
        };

        // propertyNames
        let property_names = if let Some(pn) = obj.get("propertyNames") {
            Some(Box::new(self.convert_schema(pn)?))
        } else {
            None
        };

        // minProperties / maxProperties
        let min_properties = obj
            .get("minProperties")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)));
        let max_properties = obj
            .get("maxProperties")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)));

        // dependentRequired
        let dependent_required =
            if let Some(Value::Object(dr)) = obj.get("dependentRequired") {
                let mut result = Vec::new();
                for (prop, deps) in dr {
                    if let Some(arr) = deps.as_array() {
                        let dep_names: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        if !dep_names.is_empty() {
                            result.push((prop.clone(), dep_names));
                        }
                    }
                }
                result
            } else if let Some(Value::Object(deps_map)) = obj.get("dependencies") {
                // Draft 4/7 "dependencies" — array values are dependentRequired
                let mut result = Vec::new();
                for (prop, val) in deps_map {
                    if let Some(arr) = val.as_array() {
                        let dep_names: Vec<String> = arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        if !dep_names.is_empty() {
                            result.push((prop.clone(), dep_names));
                        }
                    }
                }
                result
            } else {
                Vec::new()
            };

        // dependentSchemas
        let dependent_schemas =
            if let Some(Value::Object(ds)) = obj.get("dependentSchemas") {
                let mut result = Vec::new();
                for (prop, schema) in ds {
                    let ir = self.convert_schema(schema)?;
                    result.push((prop.clone(), ir));
                }
                result
            } else if let Some(Value::Object(deps_map)) = obj.get("dependencies") {
                // Draft 4/7 "dependencies" — object values are dependentSchemas
                let mut result = Vec::new();
                for (prop, val) in deps_map {
                    if val.is_object() || val.is_boolean() {
                        let ir = self.convert_schema(val)?;
                        result.push((prop.clone(), ir));
                    }
                }
                result
            } else {
                Vec::new()
            };

        if !has_properties && !has_pattern_props && dependent_required.is_empty()
            && dependent_schemas.is_empty() && property_names.is_none()
            && min_properties.is_none() && max_properties.is_none()
            && required_set.is_empty()
        {
            // Pure record type
            let value_type = match &additional_properties {
                AdditionalProperties::Schema(ir) => *ir.clone(),
                _ => SchemaIr::Any,
            };
            let key_type = if let Some(pn) = property_names {
                *pn
            } else {
                SchemaIr::String(StringConstraints::default())
            };
            return Ok(SchemaIr::Record(RecordSchema {
                key: Box::new(key_type),
                value: Box::new(value_type),
            }));
        }

        let mut fields: IndexMap<String, FieldSchema> = IndexMap::new();
        if let Some(Value::Object(props)) = obj.get("properties") {
            for (k, v) in props {
                let field_ir = self.convert_schema(v)?;
                fields.insert(
                    k.clone(),
                    FieldSchema {
                        schema: field_ir,
                        required: required_set.contains(k),
                    },
                );
            }
        }

        // Add required fields not in properties — JSON Schema allows this
        for req_name in &required_set {
            if !fields.contains_key(req_name) {
                fields.insert(
                    req_name.clone(),
                    FieldSchema {
                        schema: SchemaIr::Any,
                        required: true,
                    },
                );
            }
        }

        Ok(SchemaIr::Object(ObjectSchema {
            fields,
            additional_properties,
            pattern_properties,
            property_names,
            min_properties,
            max_properties,
            dependent_required,
            dependent_schemas,
        }))
    }
}

/// Walk a `serde_json::Value` using a JSON Pointer path (RFC 6901).
/// `pointer` should be the part after `#`, e.g. `"/properties/foo"` or `""` for root.
/// Returns `None` if the path cannot be resolved.
fn resolve_json_pointer(root: &Value, pointer: &str) -> Option<Value> {
    if pointer.is_empty() {
        return Some(root.clone());
    }

    // JSON Pointer must start with '/'
    if !pointer.starts_with('/') {
        return None;
    }

    let mut current = root;
    // Temporary storage so we can hold owned values between iterations
    let mut owned: Option<Value> = None;

    for raw_segment in pointer[1..].split('/') {
        // Decode JSON Pointer escapes: ~1 → /, ~0 → ~
        let segment = raw_segment.replace("~1", "/").replace("~0", "~");
        // Also percent-decode
        let segment = percent_decode(&segment);

        let next = match current {
            Value::Object(map) => map.get(&segment)?,
            Value::Array(arr) => {
                let idx: usize = segment.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };

        owned = Some(next.clone());
        // SAFETY: we just assigned to `owned`, so `as_ref().unwrap()` is safe.
        // We reborrow `current` from the cloned value stored in `owned`.
        current = owned.as_ref().unwrap();
    }

    owned
}

/// Recursively walk a JSON Schema value and register all sub-schemas that have a `$id`.
fn build_id_registry(
    schema: &Value,
    parent_base: &Option<String>,
    registry: &mut HashMap<String, Value>,
) {
    let obj = match schema.as_object() {
        Some(o) => o,
        None => return,
    };

    // Determine this schema's base URI
    let current_base = if let Some(id_str) = obj.get("$id").and_then(|v| v.as_str()) {
        let abs = if is_absolute_uri(id_str) {
            id_str.to_string()
        } else if let Some(base) = parent_base {
            resolve_relative_uri(base, id_str)
        } else {
            id_str.to_string()
        };
        // Register this sub-schema by its absolute $id
        registry.insert(abs.clone(), schema.clone());
        Some(abs)
    } else {
        parent_base.clone()
    };

    // Recurse into all sub-schema locations
    for key in &[
        "$defs",
        "definitions",
        "properties",
        "patternProperties",
        "additionalProperties",
        "items",
        "prefixItems",
        "contains",
        "if",
        "then",
        "else",
        "not",
        "propertyNames",
        "unevaluatedItems",
        "unevaluatedProperties",
    ] {
        match obj.get(*key) {
            Some(Value::Object(map)) if *key == "$defs"
                || *key == "definitions"
                || *key == "properties"
                || *key == "patternProperties" =>
            {
                for (_, v) in map {
                    build_id_registry(v, &current_base, registry);
                }
            }
            Some(Value::Array(arr)) if *key == "prefixItems" => {
                for v in arr {
                    build_id_registry(v, &current_base, registry);
                }
            }
            Some(v) if v.is_object() || v.is_boolean() => {
                build_id_registry(v, &current_base, registry);
            }
            _ => {}
        }
    }

    // Recurse into allOf, anyOf, oneOf
    for key in &["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(arr)) = obj.get(*key) {
            for v in arr {
                build_id_registry(v, &current_base, registry);
            }
        }
    }
}

/// Extract a def name from a URI (last path segment, or full URI for URNs).
fn uri_to_def_name(uri: &str) -> String {
    // For URNs, use the whole thing as the name
    if uri.starts_with("urn:") {
        return uri.to_string();
    }
    // For HTTP(S) URIs, use the last path segment (without extension)
    let without_fragment = uri.split('#').next().unwrap_or(uri);
    let without_query = without_fragment.split('?').next().unwrap_or(without_fragment);
    let last_segment = without_query.rsplit('/').next().unwrap_or(uri);
    // Strip .json extension if present
    last_segment
        .strip_suffix(".json")
        .unwrap_or(last_segment)
        .to_string()
}

/// Check if a string is an absolute URI (has a scheme).
fn is_absolute_uri(s: &str) -> bool {
    // An absolute URI starts with scheme ":" where scheme is [a-zA-Z][a-zA-Z0-9+.-]*
    if let Some(colon_pos) = s.find(':') {
        if colon_pos > 0 {
            let scheme = &s[..colon_pos];
            let mut chars = scheme.chars();
            if let Some(first) = chars.next() {
                return first.is_ascii_alphabetic()
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
            }
        }
    }
    false
}

/// Resolve a relative URI reference against a base URI.
/// Implements a simplified version of RFC 3986 Section 5.
fn resolve_relative_uri(base: &str, relative: &str) -> String {
    if is_absolute_uri(relative) {
        return relative.to_string();
    }

    // For URN schemes, relative refs don't apply — just return the relative as-is
    if base.starts_with("urn:") {
        return relative.to_string();
    }

    // Parse base: scheme://authority/path
    if relative.starts_with("//") {
        // Network-path reference — replace authority + path
        if let Some(scheme_end) = base.find("://") {
            return format!("{}{}", &base[..scheme_end + 1], relative);
        }
        return relative.to_string();
    }

    if relative.starts_with('/') {
        // Absolute-path reference — replace path
        if let Some(authority_end) = base.find("://").map(|i| {
            base[i + 3..]
                .find('/')
                .map(|j| i + 3 + j)
                .unwrap_or(base.len())
        }) {
            return format!("{}{}", &base[..authority_end], relative);
        }
        return relative.to_string();
    }

    // Relative-path reference — merge with base path
    // Remove everything after the last '/' in the base path
    let base_without_fragment = base.split('#').next().unwrap_or(base);
    if let Some(last_slash) = base_without_fragment.rfind('/') {
        format!("{}{}", &base_without_fragment[..last_slash + 1], relative)
    } else {
        relative.to_string()
    }
}

/// Collect all `SchemaIr::Ref` names (PascalCase) reachable inside an IR tree.
/// `Lazy` nodes are intentionally excluded: they represent back-edges in
/// recursive schemas and must not create dependency edges for ordering purposes.
fn collect_refs(ir: &SchemaIr) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_refs_inner(ir, &mut out);
    out
}

fn collect_refs_inner(ir: &SchemaIr, out: &mut HashSet<String>) {
    match ir {
        SchemaIr::Ref(name) => {
            out.insert(name.clone());
        }
        SchemaIr::Lazy(_) => {
            // Back-edge — skip to avoid false dependency cycles.
        }
        SchemaIr::Array(a) => {
            collect_refs_inner(&a.items, out);
            if let Some(c) = &a.contains {
                collect_refs_inner(&c.schema, out);
            }
        }
        SchemaIr::Tuple(t) => {
            for item in &t.items {
                collect_refs_inner(item, out);
            }
            if let Some(rest) = &t.rest {
                collect_refs_inner(rest, out);
            }
            if let Some(c) = &t.contains {
                collect_refs_inner(&c.schema, out);
            }
        }
        SchemaIr::Object(o) => {
            for f in o.fields.values() {
                collect_refs_inner(&f.schema, out);
            }
            match &o.additional_properties {
                AdditionalProperties::Schema(s) => collect_refs_inner(s, out),
                _ => {}
            }
            for (_, s) in &o.pattern_properties {
                collect_refs_inner(s, out);
            }
            if let Some(pn) = &o.property_names {
                collect_refs_inner(pn, out);
            }
            for (_, s) in &o.dependent_schemas {
                collect_refs_inner(s, out);
            }
        }
        SchemaIr::Record(r) => {
            collect_refs_inner(&r.key, out);
            collect_refs_inner(&r.value, out);
        }
        SchemaIr::Union(members)
        | SchemaIr::ExclusiveUnion(members)
        | SchemaIr::Intersection(members) => {
            for m in members {
                collect_refs_inner(m, out);
            }
        }
        SchemaIr::Nullable(inner)
        | SchemaIr::Optional(inner)
        | SchemaIr::Default(inner, _)
        | SchemaIr::Describe(inner, _) => {
            collect_refs_inner(inner, out);
        }
        SchemaIr::Not { base, not_schema } => {
            collect_refs_inner(base, out);
            collect_refs_inner(not_schema, out);
        }
        SchemaIr::Conditional {
            base,
            if_schema,
            then_schema,
            else_schema,
        } => {
            collect_refs_inner(base, out);
            collect_refs_inner(if_schema, out);
            if let Some(s) = then_schema {
                collect_refs_inner(s, out);
            }
            if let Some(s) = else_schema {
                collect_refs_inner(s, out);
            }
        }
        SchemaIr::TypeGuarded(guards) => {
            for (_, s) in guards {
                collect_refs_inner(s, out);
            }
        }
        // Leaves with no sub-IR
        SchemaIr::String(_)
        | SchemaIr::Number(_)
        | SchemaIr::Integer(_)
        | SchemaIr::Boolean
        | SchemaIr::Null
        | SchemaIr::Any
        | SchemaIr::Never
        | SchemaIr::Unknown
        | SchemaIr::Literal(_)
        | SchemaIr::ConstEqual(_)
        | SchemaIr::Enum(_)
        | SchemaIr::MixedEnum(_)
        | SchemaIr::ComplexEnum(_) => {}
    }
}

/// Topologically sort `def_names` so that if def A is referenced by def B,
/// A appears before B in the output.
///
/// Uses Kahn's algorithm (BFS over in-degree). If a cycle is detected (i.e.
/// not all nodes are emitted — which should not happen for non-recursive schemas
/// because recursive ones use `z.lazy()` and are excluded from `Ref` edges),
/// the remaining nodes are appended in their original order as a safe fallback.
fn topological_sort_defs(
    def_names: &[String],
    converted_defs: &HashMap<String, SchemaIr>,
) -> Vec<String> {
    // Build a map from PascalCase name → raw name for quick reverse lookup.
    let pascal_to_raw: HashMap<String, String> = def_names
        .iter()
        .map(|n| (crate::util::to_pascal_case(n), n.clone()))
        .collect();

    // Build adjacency list: raw_name → set of raw names it depends on.
    // An edge (A depends on B) means B must be emitted before A.
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();
    for name in def_names {
        let refs = converted_defs
            .get(name)
            .map(|ir| collect_refs(ir))
            .unwrap_or_default();

        // Convert PascalCase ref names back to raw names, keeping only those
        // that are actual defs (not external / unknown refs).
        let dep_set: HashSet<String> = refs
            .into_iter()
            .filter_map(|pascal| pascal_to_raw.get(&pascal).cloned())
            .filter(|dep| dep != name) // ignore self-references (handled via Lazy)
            .collect();

        deps.insert(name.clone(), dep_set);
    }

    // Kahn's algorithm.
    // in_degree[n] = number of defs that n depends on (that still haven't been emitted).
    let mut in_degree: HashMap<String, usize> = def_names
        .iter()
        .map(|n| (n.clone(), deps[n].len()))
        .collect();

    // reverse_edges[dep] = list of nodes that depend on dep.
    let mut reverse_edges: HashMap<String, Vec<String>> = HashMap::new();
    for (node, node_deps) in &deps {
        for dep in node_deps {
            reverse_edges
                .entry(dep.clone())
                .or_default()
                .push(node.clone());
        }
    }

    // Build a position lookup for stable ordering inside the queue.
    let pos: HashMap<&str, usize> = def_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    // Initialise queue with nodes that have no dependencies, preserving original
    // relative order.
    use std::collections::VecDeque;
    let mut queue: VecDeque<String> = def_names
        .iter()
        .filter(|n| in_degree[*n] == 0)
        .cloned()
        .collect();

    let mut sorted: Vec<String> = Vec::with_capacity(def_names.len());

    while let Some(node) = queue.pop_front() {
        sorted.push(node.clone());
        if let Some(dependents) = reverse_edges.get(&node) {
            // Sort dependents by their original position to keep output stable.
            let mut dependents_sorted = dependents.clone();
            dependents_sorted
                .sort_by_key(|n| pos.get(n.as_str()).copied().unwrap_or(usize::MAX));

            for dep in dependents_sorted {
                let deg = in_degree.get_mut(&dep).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(dep);
                }
            }
        }
    }

    // Fallback: if there were cycles (shouldn't happen for well-formed schemas
    // because cycles use z.lazy()), append remaining nodes in original order.
    if sorted.len() < def_names.len() {
        let emitted: HashSet<String> = sorted.iter().cloned().collect();
        let remaining: Vec<String> = def_names
            .iter()
            .filter(|n| !emitted.contains(*n))
            .cloned()
            .collect();
        sorted.extend(remaining);
    }

    sorted
}

fn extract_ref_name(ref_str: &str) -> String {
    let raw = if let Some(rest) = ref_str.strip_prefix("#/$defs/") {
        rest
    } else if let Some(rest) = ref_str.strip_prefix("#/definitions/") {
        rest
    } else {
        return String::new();
    };

    // Decode JSON Pointer escapes: ~1 → /, ~0 → ~
    // Also decode percent-encoding: %XX → char
    let decoded = raw
        .replace("~1", "/")
        .replace("~0", "~");

    // Decode percent-encoding
    percent_decode(&decoded)
}

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                    continue;
                }
            }
            result.push('%');
            result.push_str(&hex);
        } else {
            result.push(c);
        }
    }
    result
}

fn extract_nullable(members: &[Value]) -> (bool, Vec<Value>) {
    let mut nullable = false;
    let mut non_null = Vec::new();

    for m in members {
        if m.as_object()
            .and_then(|o| o.get("type"))
            .and_then(|t| t.as_str())
            == Some("null")
        {
            nullable = true;
        } else {
            non_null.push(m.clone());
        }
    }

    (nullable, non_null)
}

fn is_object_schema(schema: &Value) -> bool {
    let obj = match schema.as_object() {
        Some(o) => o,
        None => return false,
    };
    match obj.get("type").and_then(|t| t.as_str()) {
        Some("object") => return true,
        Some(_) => return false,
        None => {}
    }
    // Has properties but no type — treat as object
    obj.contains_key("properties")
}

fn parse_string_constraints(obj: &serde_json::Map<String, Value>) -> StringConstraints {
    let format = obj
        .get("format")
        .and_then(|v| v.as_str())
        .and_then(|f| match f {
            "email" | "idn-email" => Some(StringFormat::Email),
            "uri" | "url" | "iri" => Some(StringFormat::Uri),
            "uuid" => Some(StringFormat::Uuid),
            "date-time" => Some(StringFormat::DateTime),
            "date" => Some(StringFormat::Date),
            "time" => Some(StringFormat::Time),
            "duration" => Some(StringFormat::Duration),
            "ip" => Some(StringFormat::Ip),
            "ipv4" => Some(StringFormat::Ipv4),
            "ipv6" => Some(StringFormat::Ipv6),
            "hostname" | "idn-hostname" => Some(StringFormat::Hostname),
            "base64" => Some(StringFormat::Base64),
            _ => None,
        });

    StringConstraints {
        format,
        min_length: obj
            .get("minLength")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))),
        max_length: obj
            .get("maxLength")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))),
        pattern: obj
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

fn parse_number_constraints(obj: &serde_json::Map<String, Value>) -> NumberConstraints {
    let minimum = obj.get("minimum").and_then(|v| v.as_f64());
    let maximum = obj.get("maximum").and_then(|v| v.as_f64());

    // Draft 7: exclusiveMinimum/Maximum as numbers
    // Draft 4: exclusiveMinimum/Maximum as booleans (combined with minimum/maximum)
    let exclusive_minimum = match obj.get("exclusiveMinimum") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::Bool(true)) => minimum,
        _ => None,
    };

    let exclusive_maximum = match obj.get("exclusiveMaximum") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::Bool(true)) => maximum,
        _ => None,
    };

    NumberConstraints {
        minimum: if exclusive_minimum.is_some() {
            None
        } else {
            minimum
        },
        maximum: if exclusive_maximum.is_some() {
            None
        } else {
            maximum
        },
        exclusive_minimum,
        exclusive_maximum,
        multiple_of: obj.get("multipleOf").and_then(|v| v.as_f64()),
    }
}

fn const_from_value(val: &Value) -> SchemaIr {
    match val {
        Value::Object(_) | Value::Array(_) => SchemaIr::ConstEqual(val.clone()),
        _ => SchemaIr::Literal(literal_value_from_json(val)),
    }
}

fn enum_from_values(cases: &[Value]) -> SchemaIr {
    // Check if any values are objects or arrays
    let has_complex = cases
        .iter()
        .any(|v| v.is_object() || v.is_array());

    if has_complex {
        return SchemaIr::ComplexEnum(cases.to_vec());
    }

    // All primitive
    if cases.iter().all(|v| v.is_string()) {
        let strings: Vec<String> = cases
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        SchemaIr::Enum(strings)
    } else {
        let lits: Vec<LiteralValue> = cases.iter().map(literal_value_from_json).collect();
        SchemaIr::MixedEnum(lits)
    }
}

pub fn literal_value_from_json(val: &Value) -> LiteralValue {
    match val {
        Value::String(s) => LiteralValue::String(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                LiteralValue::Integer(i)
            } else {
                LiteralValue::Number(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::Bool(b) => LiteralValue::Bool(*b),
        Value::Null => LiteralValue::Null,
        _ => LiteralValue::Null,
    }
}

fn apply_modifiers(ir: SchemaIr, description: Option<String>, default_val: Option<Value>) -> SchemaIr {
    let ir = if let Some(d) = description {
        SchemaIr::Describe(Box::new(ir), d)
    } else {
        ir
    };

    if let Some(dv) = default_val {
        SchemaIr::Default(Box::new(ir), dv)
    } else {
        ir
    }
}
