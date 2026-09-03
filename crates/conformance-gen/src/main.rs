//! Walks the official JSON-Schema-Test-Suite and generates, for every test group,
//! the code for each language emitter plus a shared manifest.json that native
//! test runners consume. Conversion or emission failures are recorded in the
//! manifest rather than aborting, so runners can count them as failures.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use json_schema_transformer::input::Converter;
use json_schema_transformer::emit::{
    EmitOptions, Emitter, KotlinEmitter, PydanticEmitter, SwiftEmitter, ZodEmitter,
};
use serde::Serialize;

const NAMESPACE: &str = "spec";
const VERSION: u32 = 1;

#[derive(Parser, Debug)]
struct Args {
    /// Directory containing the suite's draft test files (e.g. tests/draft2020-12)
    #[arg(long, default_value = "JSON-Schema-Test-Suite/tests/draft2020-12")]
    suite: PathBuf,

    /// Directory of remote schema documents, mapped to http://localhost:1234/<relpath>
    #[arg(long, default_value = "JSON-Schema-Test-Suite/remotes")]
    remotes: PathBuf,

    /// Output directory for generated fixtures and manifest
    #[arg(long, default_value = "conformance/generated")]
    out: PathBuf,
}

/// The official draft 2020-12 meta-schema documents, vendored so schemas that
/// $ref the dialect (e.g. "validate against the meta-schema" tests) resolve offline.
const METASCHEMAS: &[(&str, &str)] = &[
    ("https://json-schema.org/draft/2020-12/schema", include_str!("../metaschemas/schema.json")),
    ("https://json-schema.org/draft/2020-12/meta/core", include_str!("../metaschemas/meta-core.json")),
    ("https://json-schema.org/draft/2020-12/meta/applicator", include_str!("../metaschemas/meta-applicator.json")),
    ("https://json-schema.org/draft/2020-12/meta/validation", include_str!("../metaschemas/meta-validation.json")),
    ("https://json-schema.org/draft/2020-12/meta/unevaluated", include_str!("../metaschemas/meta-unevaluated.json")),
    ("https://json-schema.org/draft/2020-12/meta/meta-data", include_str!("../metaschemas/meta-meta-data.json")),
    ("https://json-schema.org/draft/2020-12/meta/format-annotation", include_str!("../metaschemas/meta-format-annotation.json")),
    ("https://json-schema.org/draft/2020-12/meta/content", include_str!("../metaschemas/meta-content.json")),
];

fn load_remotes(dir: &std::path::Path) -> std::collections::HashMap<String, serde_json::Value> {
    let mut map = std::collections::HashMap::new();
    for (uri, text) in METASCHEMAS {
        if let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) {
            map.insert(uri.to_string(), doc);
        }
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) {
                        let rel = path.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                        map.insert(format!("http://localhost:1234/{rel}"), doc);
                    }
                }
            }
        }
    }
    map
}

#[derive(serde::Deserialize)]
struct SuiteGroup {
    description: String,
    schema: serde_json::Value,
    tests: Vec<SuiteTest>,
}

#[derive(serde::Deserialize, Serialize)]
struct SuiteTest {
    description: String,
    data: serde_json::Value,
    valid: bool,
}

#[derive(Serialize)]
struct ManifestGroup {
    /// Unique id, safe for use in file names: "<keyword>-g<index>"
    id: String,
    keyword: String,
    group_index: usize,
    description: String,
    /// Root type name in every generated language
    type_name: String,
    /// Original JSON Schema, for debugging
    schema: serde_json::Value,
    /// Per-language relative path of the generated file; absent if generation failed
    files: BTreeMap<String, String>,
    /// Per-language generation error (conversion or emitter panic); absent on success
    errors: BTreeMap<String, String>,
    tests: Vec<SuiteTest>,
}

#[derive(Serialize)]
struct Manifest {
    draft: String,
    namespace: String,
    version: u32,
    groups: Vec<ManifestGroup>,
}

fn to_pascal_case(s: &str) -> String {
    let result: String = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect();
    if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("Event{result}")
    } else {
        result
    }
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            result.push('_');
            result.push(c.to_ascii_lowercase());
        } else if !c.is_ascii_alphanumeric() && c != '_' {
            result.push('_');
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }
    result
}

fn emit_guarded(
    emitter: &dyn Emitter,
    schema: &serde_json::Value,
    name: &str,
    remotes: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<String, String> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Draft 2020-12 treats format as annotation-only; conformance measures
        // that profile (format enforcement is this library's opt-in extension)
        let converted = Converter::convert_with_options(schema, remotes, false)
            .map_err(|e| e.to_string())?;
        // Shared-helpers mode: utilities live in one companion file per language
        let options = EmitOptions {
            helpers_file: emitter.default_helpers_file().map(|s| s.to_string()),
        };
        Ok(emitter.emit_with_options(&converted, name, NAMESPACE, VERSION, &options))
    }));
    match result {
        Ok(inner) => inner,
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".to_string());
            Err(format!("panicked: {msg}"))
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let remotes = load_remotes(&args.remotes);

    let mut suite_files: Vec<PathBuf> = std::fs::read_dir(&args.suite)
        .with_context(|| format!("cannot read suite dir {}", args.suite.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file())
        .collect();
    suite_files.sort();

    let langs: Vec<(&str, Box<dyn Emitter>)> = vec![
        ("zod", Box::new(ZodEmitter)),
        ("pydantic", Box::new(PydanticEmitter)),
        ("swift", Box::new(SwiftEmitter)),
        ("kotlin", Box::new(KotlinEmitter)),
    ];

    for (lang, emitter) in &langs {
        std::fs::create_dir_all(args.out.join(lang))?;
        // Write the language's shared helpers file once
        if let (Some(filename), Some(content)) =
            (emitter.default_helpers_file(), emitter.helpers_content())
        {
            std::fs::write(args.out.join(lang).join(filename), content)?;
        }
    }

    let mut groups: Vec<ManifestGroup> = Vec::new();
    let mut error_count = 0usize;

    for file in &suite_files {
        let keyword = file.file_stem().unwrap().to_string_lossy().into_owned();
        let content = std::fs::read_to_string(file)?;
        let suite_groups: Vec<SuiteGroup> = serde_json::from_str(&content)
            .with_context(|| format!("bad suite file {}", file.display()))?;

        for (gi, group) in suite_groups.into_iter().enumerate() {
            let name = format!("{keyword}-g{gi}");
            let pascal = to_pascal_case(&name);
            let type_name = format!("{pascal}V{VERSION}Data");

            let mut files = BTreeMap::new();
            let mut errors = BTreeMap::new();

            for (lang, emitter) in &langs {
                match emit_guarded(emitter.as_ref(), &group.schema, &name, &remotes) {
                    Ok(code) => {
                        let rel = match *lang {
                            "pydantic" => format!("pydantic/{}.py", to_snake_case(&name)),
                            "swift" => format!("swift/{pascal}.swift"),
                            "kotlin" => format!("kotlin/{pascal}.kt"),
                            _ => format!("{lang}/{name}.{}", emitter.extension()),
                        };
                        std::fs::write(args.out.join(&rel), code)?;
                        files.insert(lang.to_string(), rel);
                    }
                    Err(e) => {
                        error_count += 1;
                        errors.insert(lang.to_string(), e);
                    }
                }
            }

            groups.push(ManifestGroup {
                id: name,
                keyword: keyword.clone(),
                group_index: gi,
                description: group.description,
                type_name,
                schema: group.schema,
                files,
                errors,
                tests: group.tests,
            });
        }
    }

    let manifest = Manifest {
        draft: args
            .suite
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        namespace: NAMESPACE.to_string(),
        version: VERSION,
        groups,
    };

    let manifest_path = args.out.join("manifest.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    let total = manifest.groups.len();
    let test_count: usize = manifest.groups.iter().map(|g| g.tests.len()).sum();
    println!(
        "generated {total} groups ({test_count} test cases) into {} — {error_count} generation errors recorded",
        args.out.display()
    );

    Ok(())
}
