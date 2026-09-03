use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub enum SchemaIr {
    // Primitives
    String(StringConstraints),
    Number(NumberConstraints),
    Integer(NumberConstraints),
    Boolean,
    Null,
    Any,
    Never,

    // Composites
    Array(ArraySchema),
    Object(ObjectSchema),
    Record(RecordSchema),
    Tuple(TupleSchema),

    // Set operations
    Union(Vec<SchemaIr>),
    /// oneOf — exactly one branch must match
    ExclusiveUnion(Vec<SchemaIr>),
    Intersection(Vec<SchemaIr>),

    // Literals & enums
    Literal(LiteralValue),
    /// Non-primitive const (objects, arrays) — requires deep equality check
    ConstEqual(serde_json::Value),
    /// All-string enum
    Enum(Vec<String>),
    /// Mixed primitive enum
    MixedEnum(Vec<LiteralValue>),
    /// Enum containing objects or arrays → deep equality check
    ComplexEnum(Vec<serde_json::Value>),

    // References
    Ref(String),
    Lazy(String),

    // Modifiers
    Nullable(Box<SchemaIr>),
    Optional(Box<SchemaIr>),
    Default(Box<SchemaIr>, serde_json::Value),
    Describe(Box<SchemaIr>, String),

    // Refinement wrappers
    Not {
        base: Box<SchemaIr>,
        not_schema: Box<SchemaIr>,
    },
    Conditional {
        base: Box<SchemaIr>,
        if_schema: Box<SchemaIr>,
        then_schema: Option<Box<SchemaIr>>,
        else_schema: Option<Box<SchemaIr>>,
    },

    /// Typeless schema with type-specific constraints.
    /// JSON Schema keywords only apply to their matching type; other types pass through.
    TypeGuarded(Vec<(TypeGuard, Box<SchemaIr>)>),

    /// Schema that requires full spec-interpreter validation because it uses
    /// evaluation-order-dependent or reference-resolution keywords that cannot be
    /// expressed as composable native validators (unevaluatedProperties/Items,
    /// $dynamicRef/$dynamicAnchor, $anchor, nested $id scopes, remote $refs).
    /// Emitters embed the raw schema plus remote documents and validate with an
    /// emitted, self-contained draft 2020-12 mini-validator.
    Interpreted {
        schema: serde_json::Value,
        /// URI → document, for remote references
        remotes: Vec<(String, serde_json::Value)>,
    },

    // Fallback
    Unknown,
}

#[derive(Debug, Clone)]
pub enum TypeGuard {
    String,
    Number,
    Object,
    Array,
}

#[derive(Debug, Clone)]
pub struct ConvertedSchema {
    pub defs: Vec<DefEntry>,
    pub root: SchemaIr,
}

#[derive(Debug, Clone)]
pub struct DefEntry {
    pub name: String,
    pub schema: SchemaIr,
    pub is_recursive: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StringConstraints {
    pub format: Option<StringFormat>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    pub pattern: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StringFormat {
    Email,
    Uri,
    Uuid,
    DateTime,
    Date,
    Time,
    Duration,
    Ip,
    Ipv4,
    Ipv6,
    Hostname,
    Base64,
}

#[derive(Debug, Clone, Default)]
pub struct NumberConstraints {
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub exclusive_minimum: Option<f64>,
    pub exclusive_maximum: Option<f64>,
    pub multiple_of: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ArraySchema {
    pub items: Box<SchemaIr>,
    pub min_items: Option<u64>,
    pub max_items: Option<u64>,
    pub unique_items: bool,
    pub contains: Option<ContainsSchema>,
}

#[derive(Debug, Clone)]
pub struct ContainsSchema {
    pub schema: Box<SchemaIr>,
    pub min_contains: Option<u64>,
    pub max_contains: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TupleSchema {
    pub items: Vec<SchemaIr>,
    pub rest: Option<Box<SchemaIr>>,
    pub unique_items: bool,
    pub min_items: Option<u64>,
    pub max_items: Option<u64>,
    pub contains: Option<ContainsSchema>,
}

#[derive(Debug, Clone)]
pub struct ObjectSchema {
    pub fields: IndexMap<String, FieldSchema>,
    pub additional_properties: AdditionalProperties,
    pub pattern_properties: Vec<(String, SchemaIr)>,
    pub property_names: Option<Box<SchemaIr>>,
    pub min_properties: Option<u64>,
    pub max_properties: Option<u64>,
    pub dependent_required: Vec<(String, Vec<String>)>,
    pub dependent_schemas: Vec<(String, SchemaIr)>,
}

#[derive(Debug, Clone)]
pub struct RecordSchema {
    pub key: Box<SchemaIr>,
    pub value: Box<SchemaIr>,
}

#[derive(Debug, Clone)]
pub struct FieldSchema {
    pub schema: SchemaIr,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub enum AdditionalProperties {
    Allowed,
    Forbidden,
    Schema(Box<SchemaIr>),
}

#[derive(Debug, Clone)]
pub enum LiteralValue {
    String(String),
    Number(f64),
    Integer(i64),
    Bool(bool),
    Null,
}

/// Check if a SchemaIr tree uses uniqueItems (needs _stableStringify helper)
pub fn has_unique_items(ir: &SchemaIr) -> bool {
    match ir {
        SchemaIr::Array(a) => a.unique_items || a.contains.as_ref().is_some_and(|c| has_unique_items(&*c.schema)),
        SchemaIr::Tuple(t) => t.unique_items || t.items.iter().any(has_unique_items)
            || t.rest.as_ref().is_some_and(|r| has_unique_items(r)),
        SchemaIr::Object(o) => o.fields.values().any(|f| has_unique_items(&f.schema)),
        SchemaIr::Record(r) => has_unique_items(&r.key) || has_unique_items(&r.value),
        SchemaIr::Union(members) | SchemaIr::ExclusiveUnion(members) | SchemaIr::Intersection(members) => {
            members.iter().any(has_unique_items)
        }
        SchemaIr::Nullable(inner) | SchemaIr::Optional(inner) | SchemaIr::Default(inner, _)
            | SchemaIr::Describe(inner, _) => has_unique_items(inner),
        SchemaIr::Not { base, not_schema } => has_unique_items(base) || has_unique_items(not_schema),
        SchemaIr::Conditional { base, if_schema, then_schema, else_schema } =>
            has_unique_items(base) || has_unique_items(if_schema)
                || then_schema.as_ref().is_some_and(|s| has_unique_items(s))
                || else_schema.as_ref().is_some_and(|s| has_unique_items(s)),
        SchemaIr::TypeGuarded(guards) => guards.iter().any(|(_, s)| has_unique_items(s)),
        _ => false,
    }
}

/// Check if a SchemaIr tree contains ConstEqual or ComplexEnum (needs _deepEqual helper)
pub fn needs_deep_equal(ir: &SchemaIr) -> bool {
    match ir {
        SchemaIr::ConstEqual(_) | SchemaIr::ComplexEnum(_) => true,
        SchemaIr::Array(a) => needs_deep_equal(&a.items)
            || a.contains.as_ref().is_some_and(|c| needs_deep_equal(&*c.schema)),
        SchemaIr::Object(o) => o.fields.values().any(|f| needs_deep_equal(&f.schema))
            || o.dependent_schemas.iter().any(|(_, s)| needs_deep_equal(s))
            || o.pattern_properties.iter().any(|(_, s)| needs_deep_equal(s)),
        SchemaIr::Record(r) => needs_deep_equal(&r.key) || needs_deep_equal(&r.value),
        SchemaIr::Tuple(t) => t.items.iter().any(needs_deep_equal)
            || t.rest.as_ref().is_some_and(|r| needs_deep_equal(r)),
        SchemaIr::Union(members) | SchemaIr::ExclusiveUnion(members) | SchemaIr::Intersection(members) => {
            members.iter().any(needs_deep_equal)
        }
        SchemaIr::Nullable(inner) | SchemaIr::Optional(inner) | SchemaIr::Default(inner, _)
            | SchemaIr::Describe(inner, _) => needs_deep_equal(inner),
        SchemaIr::Not { base, not_schema } => needs_deep_equal(base) || needs_deep_equal(not_schema),
        SchemaIr::Conditional { base, if_schema, then_schema, else_schema } =>
            needs_deep_equal(base) || needs_deep_equal(if_schema)
                || then_schema.as_ref().is_some_and(|s| needs_deep_equal(s))
                || else_schema.as_ref().is_some_and(|s| needs_deep_equal(s)),
        SchemaIr::TypeGuarded(guards) => guards.iter().any(|(_, s)| needs_deep_equal(s)),
        _ => false,
    }
}
