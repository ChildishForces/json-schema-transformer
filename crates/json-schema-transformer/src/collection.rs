use crate::emit::{EmitOptions, Emitter, HelperSet, SharedHelpers};
use crate::input::{ConvertError, Converter};
use crate::ir::ConvertedSchema;

/// Emits a collection of schemas that share one tailored helpers file.
///
/// Each [`CollectionSession::emit`] call produces one module in collection
/// mode (helpers referenced, never inlined) and accumulates the helper
/// components that module needs. After all members are emitted,
/// [`CollectionSession::helpers`] yields the shared helpers file containing
/// exactly the union of accumulated needs — or None when no member needed
/// any helpers.
pub struct CollectionSession<'e> {
    emitter: &'e dyn Emitter,
    helpers_file: String,
    needs: HelperSet,
    mutable: bool,
}

impl<'e> CollectionSession<'e> {
    /// Session using the emitter's conventional helpers file name. For
    /// helper-less languages (plain TypeScript) the name is unused and
    /// [`CollectionSession::helpers`] always returns None.
    pub fn new(emitter: &'e dyn Emitter) -> Self {
        let helpers_file = emitter
            .default_helpers_file()
            .unwrap_or_default()
            .to_string();
        Self {
            emitter,
            helpers_file,
            needs: HelperSet::default(),
            mutable: false,
        }
    }

    /// Session with a custom helpers file name.
    pub fn with_helpers_file(emitter: &'e dyn Emitter, file_name: impl Into<String>) -> Self {
        Self {
            emitter,
            helpers_file: file_name.into(),
            needs: HelperSet::default(),
            mutable: false,
        }
    }

    /// Emit mutable stored properties (see [`EmitOptions::mutable`]).
    pub fn mutable(mut self, mutable: bool) -> Self {
        self.mutable = mutable;
        self
    }

    /// Emit one member module. `dir_prefix` is the relative path from the
    /// module's output directory back to the collection root: "" for
    /// root-level files, one "../" per nesting level.
    pub fn emit(
        &mut self,
        schema: &serde_json::Value,
        name: Option<&str>,
        dir_prefix: &str,
    ) -> Result<String, ConvertError> {
        let resolved = crate::resolve_name(schema, name)?;
        let converted = Converter::convert(schema)?;
        Ok(self.emit_converted(&converted, &resolved, dir_prefix))
    }

    /// Like [`CollectionSession::emit`] for a pre-converted schema (callers
    /// that need `convert_with_remotes`/format options).
    pub fn emit_converted(
        &mut self,
        converted: &ConvertedSchema,
        name: &str,
        dir_prefix: &str,
    ) -> String {
        let options = EmitOptions {
            helpers: Some(SharedHelpers {
                file_name: self.helpers_file.clone(),
                dir_prefix: dir_prefix.to_string(),
            }),
            mutable: self.mutable,
        };
        let (code, needs) = self.emitter.emit_collecting(converted, name, &options);
        self.needs.union_with(&needs);
        code
    }

    /// Union of helper components needed by all members emitted so far.
    pub fn helper_needs(&self) -> &HelperSet {
        &self.needs
    }

    /// (file name, tailored content) for the shared helpers file, or None
    /// when no member needed helpers (no file should be written).
    pub fn helpers(&self) -> Option<(String, String)> {
        let content = self.emitter.helpers_content_for(&self.needs)?;
        Some((self.helpers_file.clone(), content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unique_items_schema() -> serde_json::Value {
        json!({"type": "array", "items": {"type": "integer"}, "uniqueItems": true})
    }

    fn interpreted_schema() -> serde_json::Value {
        json!({"type": "object", "unevaluatedProperties": false})
    }

    fn plain_schema() -> serde_json::Value {
        json!({"type": "object", "properties": {"id": {"type": "string"}}})
    }

    #[cfg(feature = "zod")]
    #[test]
    fn zod_tailors_helpers_to_needed_components() {
        let mut session = CollectionSession::new(&crate::ZodEmitter);
        session.emit(&unique_items_schema(), Some("Sample"), "").unwrap();
        let (file, content) = session.helpers().unwrap();
        assert_eq!(file, "jst-helpers.ts");
        assert!(content.contains("_stableStringify"));
        assert!(!content.contains("_jsiValidate"));
        assert!(!content.contains("_deepEqual"));
    }

    #[cfg(feature = "zod")]
    #[test]
    fn zod_helper_free_collection_has_no_helpers_file() {
        let mut session = CollectionSession::new(&crate::ZodEmitter);
        session.emit(&plain_schema(), Some("Sample"), "").unwrap();
        assert!(session.helper_needs().is_empty());
        assert!(session.helpers().is_none());
    }

    #[cfg(feature = "zod")]
    #[test]
    fn zod_accumulates_union_across_members() {
        let mut session = CollectionSession::new(&crate::ZodEmitter);
        session.emit(&unique_items_schema(), Some("A"), "").unwrap();
        session.emit(&interpreted_schema(), Some("B"), "").unwrap();
        let (_, content) = session.helpers().unwrap();
        assert!(content.contains("_stableStringify"));
        assert!(content.contains("_jsiValidate"));
    }

    #[cfg(feature = "zod")]
    #[test]
    fn zod_nested_member_imports_with_depth_prefix() {
        let mut session = CollectionSession::new(&crate::ZodEmitter);
        let nested = session
            .emit(&unique_items_schema(), Some("Nested"), "../")
            .unwrap();
        assert!(nested.contains("from \"../jst-helpers\""));
        let root = session
            .emit(&unique_items_schema(), Some("Root"), "")
            .unwrap();
        assert!(root.contains("from \"./jst-helpers\""));
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_interpreter_pulls_equality_helper() {
        let mut session = CollectionSession::new(&crate::KotlinEmitter);
        session.emit(&interpreted_schema(), Some("Sample"), "").unwrap();
        let (file, content) = session.helpers().unwrap();
        assert_eq!(file, "JstHelpers.kt");
        assert!(content.contains("_jsiValidate"));
        assert!(content.contains("_jsiEq"));
    }

    #[cfg(feature = "swift")]
    #[test]
    fn swift_tailors_validation_error_to_union_of_cases() {
        let mut session = CollectionSession::new(&crate::SwiftEmitter);
        session
            .emit(
                &json!({"type": "string", "format": "email"}),
                Some("Sample"),
                "",
            )
            .unwrap();
        let (file, content) = session.helpers().unwrap();
        assert_eq!(file, "JstHelpers.swift");
        assert!(content.contains("case invalidEmail(String)"));
        assert!(!content.contains("case invalidTime(String)"));
        assert!(!content.contains("private enum ValidationError"));
        assert!(content.contains("enum ValidationError"));
    }

    #[cfg(feature = "pydantic")]
    #[test]
    fn pydantic_mutable_enables_assignment_validation() {
        let mut session = CollectionSession::new(&crate::PydanticEmitter).mutable(true);
        let code = session.emit(&plain_schema(), Some("Sample"), "").unwrap();
        assert!(code.contains("\"validate_assignment\": True"));

        let mut immutable = CollectionSession::new(&crate::PydanticEmitter);
        let code = immutable.emit(&plain_schema(), Some("Sample"), "").unwrap();
        assert!(!code.contains("validate_assignment"));
    }

    #[cfg(feature = "pydantic")]
    #[test]
    fn pydantic_tailors_type_check_defs() {
        let mut session = CollectionSession::new(&crate::PydanticEmitter);
        session.emit(&unique_items_schema(), Some("Sample"), "").unwrap();
        let (file, content) = session.helpers().unwrap();
        assert_eq!(file, "jst_helpers.py");
        assert!(content.contains("def _check_int("));
        assert!(!content.contains("def _check_str("));
        assert!(!content.contains("def _jsi_validate("));
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_never_produces_helpers() {
        let mut session = CollectionSession::new(&crate::TypeScriptEmitter);
        session.emit(&interpreted_schema(), Some("Sample"), "").unwrap();
        assert!(session.helpers().is_none());
    }
}
