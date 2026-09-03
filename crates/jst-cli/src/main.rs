use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, ValueEnum};
use json_schema_transformer::emit::EmitOptions;
use json_schema_transformer::input::Converter;
use json_schema_transformer::{
    Emitter, KotlinEmitter, PydanticEmitter, SwiftEmitter, TypeScriptEmitter, ZodEmitter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Target {
    Zod,
    Typescript,
    Pydantic,
    Swift,
    Kotlin,
}

impl Target {
    fn all() -> Vec<Target> {
        vec![
            Target::Zod,
            Target::Typescript,
            Target::Pydantic,
            Target::Swift,
            Target::Kotlin,
        ]
    }

    fn emitter(&self) -> Box<dyn Emitter> {
        match self {
            Target::Zod => Box::new(ZodEmitter),
            Target::Typescript => Box::new(TypeScriptEmitter),
            Target::Pydantic => Box::new(PydanticEmitter),
            Target::Swift => Box::new(SwiftEmitter),
            Target::Kotlin => Box::new(KotlinEmitter),
        }
    }
}

/// Convert a JSON Schema into native types and validation schemas.
#[derive(Parser, Debug)]
#[command(name = "jst", version, about)]
struct Args {
    /// Path to the JSON Schema file, or "-" to read from stdin
    schema: PathBuf,

    /// Output target(s); repeat for multiple. Defaults to all targets when --out-dir is set,
    /// otherwise exactly one target must be given.
    #[arg(short, long, value_enum)]
    target: Vec<Target>,

    /// Logical schema name used in generated type names (default: schema file stem)
    #[arg(short, long)]
    name: Option<String>,

    /// Namespace recorded in the generated file header
    #[arg(long, default_value = "schemas")]
    namespace: String,

    /// Schema version, embedded in generated type names
    #[arg(long, default_value_t = 1)]
    schema_version: u32,

    /// Write output file(s) to this directory as <name>.<ext>
    #[arg(short = 'd', long)]
    out_dir: Option<PathBuf>,

    /// Write single-target output to this file (default: stdout)
    #[arg(short, long, conflicts_with = "out_dir")]
    out: Option<PathBuf>,

    /// Emit shared utility helpers (validators, wrapper types) into a separate
    /// companion file instead of inlining them into each schema module. Pass a
    /// filename to override the per-language default (e.g. jst-helpers.ts).
    /// Requires --out or --out-dir.
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "FILENAME")]
    helpers_file: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let raw = if args.schema.as_os_str() == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read schema from stdin")?;
        buf
    } else {
        std::fs::read_to_string(&args.schema)
            .with_context(|| format!("failed to read {}", args.schema.display()))?
    };

    let schema: serde_json::Value = serde_json::from_str(&raw).context("invalid JSON")?;

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            if args.schema.as_os_str() == "-" {
                "schema".to_string()
            } else {
                args.schema
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "schema".to_string())
            }
        }
    };

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
    if args.helpers_file.is_some() && args.out.is_none() && args.out_dir.is_none() {
        bail!("--helpers-file requires --out or --out-dir");
    }
    if let Some(custom) = &args.helpers_file {
        if !custom.is_empty() && targets.len() > 1 {
            bail!("a custom --helpers-file name only makes sense with a single target; omit the value to use per-language defaults");
        }
    }

    let converted = Converter::convert(&schema).context("conversion failed")?;
    let helper_dir = args
        .out_dir
        .clone()
        .or_else(|| args.out.as_ref().and_then(|o| o.parent().map(|p| p.to_path_buf())));

    for target in &targets {
        let emitter = target.emitter();

        // Resolve helper-file name for this emitter (custom, or language default)
        let helpers_file = match &args.helpers_file {
            Some(custom) if !custom.is_empty() => Some(custom.clone()),
            Some(_) => emitter.default_helpers_file().map(|s| s.to_string()),
            None => None,
        };
        let options = EmitOptions { helpers_file: helpers_file.clone() };

        let output = emitter.emit_with_options(
            &converted,
            &name,
            &args.namespace,
            args.schema_version,
            &options,
        );

        if let (Some(filename), Some(content)) = (&helpers_file, emitter.helpers_content()) {
            let dir = helper_dir.clone().unwrap_or_else(|| PathBuf::from("."));
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
            let path = dir.join(filename);
            std::fs::write(&path, &content)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }

        if let Some(dir) = &args.out_dir {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
            let path = dir.join(format!("{name}.{}", emitter.extension()));
            std::fs::write(&path, &output)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        } else if let Some(out) = &args.out {
            std::fs::write(out, &output)
                .with_context(|| format!("failed to write {}", out.display()))?;
        } else {
            print!("{output}");
        }
    }

    Ok(())
}
