use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Parser, ValueEnum};
use json_schema_transformer::emit::EmitOptions;
use json_schema_transformer::input::Converter;
use json_schema_transformer::util::{to_pascal_case, to_snake_case};
use json_schema_transformer::{
    CollectionSession, Emitter, KotlinEmitter, PydanticEmitter, RustEmitter, SwiftEmitter,
    TypeScriptEmitter, ZodEmitter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Target {
    Zod,
    Typescript,
    Pydantic,
    Swift,
    Kotlin,
    Rust,
}

impl Target {
    fn all() -> Vec<Target> {
        vec![
            Target::Zod,
            Target::Typescript,
            Target::Pydantic,
            Target::Swift,
            Target::Kotlin,
            Target::Rust,
        ]
    }

    fn emitter(&self) -> Box<dyn Emitter> {
        match self {
            Target::Zod => Box::new(ZodEmitter),
            Target::Typescript => Box::new(TypeScriptEmitter),
            Target::Pydantic => Box::new(PydanticEmitter),
            Target::Swift => Box::new(SwiftEmitter),
            Target::Kotlin => Box::new(KotlinEmitter),
            Target::Rust => Box::new(RustEmitter),
        }
    }

    /// File stem for collection-mode output, following each language's
    /// module-naming convention. Snake-cased stems go through PascalCase
    /// first so multi-word names collapse cleanly ("Order Item" → order_item).
    fn stem(&self, name: &str) -> String {
        match self {
            Target::Pydantic | Target::Rust => to_snake_case(&to_pascal_case(name)),
            _ => to_pascal_case(name),
        }
    }
}

/// Convert JSON Schemas into native types and validation schemas.
///
/// Runs in one of two modes:
///
/// Single-file mode (one schema file or "-", output to --out or stdout):
/// the generated module is self-contained — all runtime helpers are inlined.
///
/// Collection mode (--out-dir, multiple schemas, or a directory input):
/// one output file per schema, mirroring the input directory structure, plus
/// one shared helpers file at the output root containing exactly the helpers
/// the emitted modules need.
#[derive(Parser, Debug)]
#[command(name = "jst", version, about)]
struct Args {
    /// Schema files and/or directories (recursively expanded to **/*.json),
    /// or "-" to read a single schema from stdin
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Output target(s); repeat for multiple. Defaults to all targets when --out-dir is set,
    /// otherwise exactly one target must be given.
    #[arg(short, long, value_enum)]
    target: Vec<Target>,

    /// Name used for generated type names (default: the schema's root "title",
    /// falling back to the input file stem). Requires exactly one schema.
    #[arg(short, long)]
    name: Option<String>,

    /// Write output file(s) to this directory (collection mode)
    #[arg(short = 'd', long)]
    out_dir: Option<PathBuf>,

    /// Write single-target output to this file (default: stdout)
    #[arg(short, long, conflicts_with = "out_dir")]
    out: Option<PathBuf>,

    /// Override the shared helpers file name (collection mode, single target)
    #[arg(long, value_name = "FILENAME")]
    helpers_file: Option<String>,

    /// Emit mutable stored properties (Swift/Kotlin `var`; Pydantic
    /// assignment re-validation). Validation APIs are emitted regardless.
    #[arg(long)]
    mutable: bool,
}

/// One schema to convert: where it came from, its parsed contents, the
/// directory it maps to relative to the output root, and its resolved name.
struct SchemaInput {
    source: String,
    schema: serde_json::Value,
    rel_dir: PathBuf,
    name: String,
}

fn read_schema(path: &Path) -> anyhow::Result<serde_json::Value> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn schema_title(schema: &serde_json::Value) -> Option<String> {
    schema
        .as_object()
        .and_then(|o| o.get("title"))
        .and_then(|t| t.as_str())
        .map(|t| t.to_string())
}

/// Recursively collect *.json files under `dir`, sorted for determinism.
/// Symlinked directories are not followed (prevents cycles and duplicated
/// output); symlinked .json files are included.
fn walk_json_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("failed to read directory {}", current.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("failed to stat {}", path.display()))?;
            let is_json = path.extension().is_some_and(|e| e == "json");
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_symlink() {
                // Resolve the link target's type without recursing into
                // symlinked directories.
                let meta = std::fs::metadata(&path)
                    .with_context(|| format!("failed to resolve symlink {}", path.display()))?;
                if meta.is_file() && is_json {
                    files.push(path);
                }
            } else if is_json {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let is_stdin = args.inputs.iter().any(|p| p.as_os_str() == "-");
    if is_stdin && args.inputs.len() > 1 {
        bail!("\"-\" (stdin) cannot be combined with other inputs");
    }
    let has_dir_input = !is_stdin && args.inputs.iter().any(|p| p.is_dir());

    // Expand inputs: files stay at the output root; each directory's *.json
    // files keep their position relative to that directory.
    let mut inputs: Vec<SchemaInput> = Vec::new();
    if is_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read schema from stdin")?;
        let schema: serde_json::Value = serde_json::from_str(&buf).context("invalid JSON")?;
        let Some(name) = args.name.clone().or_else(|| schema_title(&schema)) else {
            bail!("cannot determine a name: pass --name, or add a root \"title\" to the schema");
        };
        inputs.push(SchemaInput {
            source: "<stdin>".to_string(),
            schema,
            rel_dir: PathBuf::new(),
            name,
        });
    } else {
        for input in &args.inputs {
            if input.is_dir() {
                for path in walk_json_files(input)? {
                    let schema = read_schema(&path)?;
                    let rel = path.strip_prefix(input).expect("walked file under input dir");
                    let rel_dir = rel.parent().map(Path::to_path_buf).unwrap_or_default();
                    let name = schema_title(&schema)
                        .or_else(|| {
                            path.file_stem().map(|s| s.to_string_lossy().into_owned())
                        })
                        .expect("file stem always present for walked files");
                    inputs.push(SchemaInput {
                        source: path.display().to_string(),
                        schema,
                        rel_dir,
                        name,
                    });
                }
            } else {
                let schema = read_schema(input)?;
                let name = args
                    .name
                    .clone()
                    .or_else(|| schema_title(&schema))
                    .or_else(|| input.file_stem().map(|s| s.to_string_lossy().into_owned()));
                let Some(name) = name else {
                    bail!(
                        "cannot determine a name for {}: pass --name, or add a root \"title\"",
                        input.display()
                    );
                };
                inputs.push(SchemaInput {
                    source: input.display().to_string(),
                    schema,
                    rel_dir: PathBuf::new(),
                    name,
                });
            }
        }
    }
    if inputs.is_empty() {
        bail!("no .json schema files found in the given inputs");
    }

    // --name applies whenever exactly one schema results — including a
    // directory input containing a single file (previously silently ignored).
    if let Some(name) = &args.name {
        if inputs.len() > 1 {
            bail!("--name is ambiguous with multiple schemas; use per-schema \"title\"s");
        }
        inputs[0].name = name.clone();
    }

    let collection = args.out_dir.is_some() || inputs.len() > 1 || has_dir_input;
    if collection && args.out_dir.is_none() {
        bail!("multiple schemas or directory inputs require --out-dir");
    }
    if !collection && args.helpers_file.is_some() {
        bail!("--helpers-file only applies in collection mode (--out-dir); single-file output always inlines helpers");
    }

    let targets = if args.target.is_empty() {
        if args.out_dir.is_some() {
            Target::all()
        } else {
            bail!("specify --target, or --out-dir to generate all targets");
        }
    } else {
        args.target.clone()
    };
    if targets.len() > 1 && args.out_dir.is_none() {
        bail!("multiple targets require --out-dir");
    }
    if let Some(custom) = &args.helpers_file {
        if targets.len() > 1 {
            bail!("a custom --helpers-file name only makes sense with a single target");
        }
        // The name lands in imports (`from {stem} import`, `use {stem}::*`)
        // — validate it up front instead of emitting broken code.
        let emitter = targets[0].emitter();
        let Some(default) = emitter.default_helpers_file() else {
            bail!("this target emits no runtime helpers; --helpers-file does not apply");
        };
        let expected_ext = default.rsplit('.').next().unwrap_or_default();
        let Some(stem) = custom.strip_suffix(&format!(".{expected_ext}")) else {
            bail!("--helpers-file for this target must end with .{expected_ext} (e.g. {default})");
        };
        let module_safe = !stem.is_empty()
            && stem
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let needs_identifier = matches!(targets[0], Target::Pydantic | Target::Rust);
        if !module_safe || (needs_identifier && stem.contains('-')) {
            bail!(
                "--helpers-file stem \"{stem}\" is not importable for this target; use letters, digits and underscores"
            );
        }
    }

    if !collection {
        // Single-file mode: one self-contained module to --out or stdout.
        let input = &inputs[0];
        let converted = Converter::convert(&input.schema).context("conversion failed")?;
        let options = EmitOptions::new().with_mutable(args.mutable);
        for target in &targets {
            let emitter = target.emitter();
            let output = emitter.emit_with_options(&converted, &input.name, &options);
            if let Some(out) = &args.out {
                std::fs::write(out, &output)
                    .with_context(|| format!("failed to write {}", out.display()))?;
            } else {
                print!("{output}");
            }
        }
        return Ok(());
    }

    // Collection mode: mirrored per-schema files + one tailored helpers file
    // per target at the output root.
    let out_dir = args.out_dir.as_ref().expect("checked above");
    for target in &targets {
        let emitter = target.emitter();
        let mut session = match &args.helpers_file {
            Some(custom) => CollectionSession::with_helpers_file(emitter.as_ref(), custom.clone()),
            None => CollectionSession::new(emitter.as_ref()),
        }
        .mutable(args.mutable);

        // Pre-flight: compute and collision-check every output path before
        // writing anything. Paths are compared case-insensitively because
        // common filesystems (APFS, NTFS) are — two stems differing only in
        // case would silently overwrite each other. The shared helpers file
        // name is reserved up front so a schema can't clobber it (or vice
        // versa).
        let mut planned: Vec<(PathBuf, &SchemaInput)> = Vec::new();
        let mut claimed: HashMap<String, String> = HashMap::new();
        for input in &inputs {
            let path = out_dir
                .join(&input.rel_dir)
                .join(format!("{}.{}", target.stem(&input.name), emitter.extension()));
            let key = path.to_string_lossy().to_lowercase();
            if let Some(previous) = claimed.insert(key, input.source.clone()) {
                bail!(
                    "output collision: {} would be written by both {} and {} (paths are compared case-insensitively)",
                    path.display(),
                    previous,
                    input.source
                );
            }
            planned.push((path, input));
        }
        if let Some(helpers_name) = session.helpers_file_name() {
            let helpers_path = out_dir.join(helpers_name);
            let key = helpers_path.to_string_lossy().to_lowercase();
            if let Some(previous) = claimed.get(&key) {
                bail!(
                    "output collision: the shared helpers file {} would clash with the module generated from {}; rename the schema or pass a different --helpers-file",
                    helpers_path.display(),
                    previous
                );
            }
        }

        for (path, input) in &planned {
            let converted = Converter::convert(&input.schema)
                .with_context(|| format!("conversion failed for {}", input.source))?;
            let dir_prefix = "../".repeat(input.rel_dir.components().count());
            let output = session.emit_converted(&converted, &input.name, &dir_prefix);

            let dir = path.parent().expect("output path has a parent");
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
            std::fs::write(path, &output)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }

        if let Some((file_name, content)) = session.helpers() {
            let path = out_dir.join(&file_name);
            std::fs::write(&path, &content)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
    }

    Ok(())
}
