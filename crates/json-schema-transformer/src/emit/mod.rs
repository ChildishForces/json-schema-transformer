#[cfg(feature = "kotlin")]
pub mod kotlin;
#[cfg(feature = "pydantic")]
pub mod pydantic;
#[cfg(feature = "rust")]
pub mod rust;
#[cfg(feature = "swift")]
pub mod swift;
#[cfg(feature = "typescript")]
pub mod typescript;
#[cfg(feature = "zod")]
pub mod zod;

use crate::ir::ConvertedSchema;

/// A set of helper-component keys a generated module needs. Keys are opaque,
/// per-language identifiers defined by each emitter (e.g. "deep_equal",
/// "email", "jsi"); [`Emitter::helpers_content_for`] maps a union of them
/// back to a tailored shared-helpers file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelperSet(pub std::collections::BTreeSet<&'static str>);

impl HelperSet {
    pub fn insert(&mut self, key: &'static str) {
        self.0.insert(key);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains(key)
    }

    pub fn union_with(&mut self, other: &HelperSet) {
        self.0.extend(other.0.iter());
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Options controlling module emission.
///
/// Marked `#[non_exhaustive]` so future option fields are not breaking
/// changes: construct via [`EmitOptions::default`] (or [`EmitOptions::new`])
/// plus the `with_*` builder methods.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// None → single-file mode: utility helpers (deep-equality, canonical
    /// stringify, the spec-interpreter, format wrapper types, ...) are
    /// inlined so the module is self-contained.
    ///
    /// Some → collection mode: helpers live in a shared companion file and
    /// the module references them instead: an import in TypeScript/Python, a
    /// `use` in Rust, same-module internal declarations in Swift/Kotlin.
    /// Generate the companion file once via [`Emitter::helpers_content_for`]
    /// with the union of needs reported by [`Emitter::emit_collecting`].
    pub helpers: Option<SharedHelpers>,

    /// Emit mutable stored properties (Swift `var` instead of `let`, Kotlin
    /// `var` instead of `val`; Pydantic enables assignment re-validation).
    /// Validation APIs (`validate()`, manual constructors) are emitted
    /// regardless — this only affects field mutability. No-op for languages
    /// whose output is already mutable (Rust, TypeScript/Zod).
    pub mutable: bool,
}

impl EmitOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Collection mode: reference helpers from a shared companion file.
    pub fn with_helpers(mut self, helpers: Option<SharedHelpers>) -> Self {
        self.helpers = helpers;
        self
    }

    /// Emit mutable stored properties (see the `mutable` field).
    pub fn with_mutable(mut self, mutable: bool) -> Self {
        self.mutable = mutable;
        self
    }
}

/// Where the shared helpers file lives relative to a generated module.
#[derive(Debug, Clone)]
pub struct SharedHelpers {
    /// Helper file name at the collection root, e.g. "jst-helpers.ts".
    pub file_name: String,
    /// Relative path prefix from the generated file's directory back to the
    /// collection root: "" for root-level files, one "../" per nesting level.
    /// Always ends with '/' when non-empty.
    pub dir_prefix: String,
}

impl SharedHelpers {
    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            dir_prefix: String::new(),
        }
    }

    /// Set the relative path prefix from the generated file's directory back
    /// to the collection root ("" or one "../" per nesting level).
    pub fn with_dir_prefix(mut self, dir_prefix: impl Into<String>) -> Self {
        self.dir_prefix = dir_prefix.into();
        self
    }

    /// Number of directory levels between the generated file and the
    /// collection root, derived from `dir_prefix`.
    pub fn depth(&self) -> usize {
        self.dir_prefix.matches("../").count()
    }
}

/// Trait for emitting generated code from a converted JSON Schema.
pub trait Emitter {
    /// File extension for the generated output (e.g., "zod.ts", "d.ts", "py").
    fn extension(&self) -> &str;

    /// Generate a self-contained output module (helpers inlined). The name is
    /// PascalCased into the root type name.
    fn emit(&self, converted: &ConvertedSchema, name: &str) -> String {
        self.emit_with_options(converted, name, &EmitOptions::default())
    }

    /// Generate the output module honoring [`EmitOptions`].
    fn emit_with_options(
        &self,
        converted: &ConvertedSchema,
        name: &str,
        options: &EmitOptions,
    ) -> String;

    /// Collection mode: emit and report which shared-helper components this
    /// module references. The default suits languages without runtime helpers.
    fn emit_collecting(
        &self,
        converted: &ConvertedSchema,
        name: &str,
        options: &EmitOptions,
    ) -> (String, HelperSet) {
        (
            self.emit_with_options(converted, name, options),
            HelperSet::default(),
        )
    }

    /// Shared helpers file tailored to exactly `needs` plus internal
    /// dependencies (e.g. Kotlin's interpreter pulls in its equality helper).
    /// None when `needs` is empty or the language emits no runtime helpers.
    fn helpers_content_for(&self, needs: &HelperSet) -> Option<String> {
        if needs.is_empty() {
            None
        } else {
            self.helpers_content()
        }
    }

    /// Full content of this language's shared helpers file (a superset of
    /// every possible need). Fallback for [`Emitter::helpers_content_for`].
    /// None when the language emits no runtime helpers.
    fn helpers_content(&self) -> Option<String> {
        None
    }

    /// Conventional file name for the shared helpers file.
    fn default_helpers_file(&self) -> Option<&'static str> {
        None
    }
}

// Re-export emitter types
#[cfg(feature = "kotlin")]
pub use kotlin::KotlinEmitter;
#[cfg(feature = "pydantic")]
pub use pydantic::PydanticEmitter;
#[cfg(feature = "rust")]
pub use rust::RustEmitter;
#[cfg(feature = "swift")]
pub use swift::SwiftEmitter;
#[cfg(feature = "typescript")]
pub use typescript::TypeScriptEmitter;
#[cfg(feature = "zod")]
pub use zod::ZodEmitter;

// Re-export zod_for for backward compatibility with tests
#[cfg(feature = "zod")]
pub use zod::zod_for;

/// Shared helper: check if a JS/TS identifier needs quoting.
pub fn needs_quoting(key: &str) -> bool {
    if key.is_empty() || key == "__proto__" {
        return true;
    }
    let mut chars = key.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' && first != '$' {
        return true;
    }
    chars.any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
}

/// Shared helper: check if a Python identifier needs special handling.
pub fn needs_python_quoting(key: &str) -> bool {
    const PYTHON_KEYWORDS: &[&str] = &[
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
        "return", "try", "while", "with", "yield",
    ];
    if key.is_empty() {
        return true;
    }
    if PYTHON_KEYWORDS.contains(&key) {
        return true;
    }
    let mut chars = key.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return true;
    }
    chars.any(|c| !c.is_ascii_alphanumeric() && c != '_')
}

/// Sanitize schema-derived text (titles, descriptions) for embedding in
/// generated comments: strips carriage returns (a bare CR is a hard error in
/// Rust doc comments), Unicode bidi-control codepoints (deny-by-default rustc
/// lint, and a source-spoofing hazard in any language), and other C0 controls
/// except newline/tab.
pub fn sanitize_comment_text(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(
                c,
                '\r' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
            ) && (*c == '\n' || *c == '\t' || !c.is_control())
        })
        .collect()
}

/// Generate a module header comment. The name is flattened to a single line
/// and sanitized — schema titles are arbitrary strings.
pub fn module_header(name: &str) -> String {
    let name = sanitize_comment_text(name).replace('\n', " ");
    format!(
        "// Generated by json-schema-transformer\n// {name}\n// DO NOT EDIT — changes will be overwritten on regeneration\n"
    )
}
