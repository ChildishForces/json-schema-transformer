use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, ValueEnum};
use json_schema_transformer::{
    Emitter, KotlinEmitter, PydanticEmitter, SwiftEmitter, TypeScriptEmitter, ZodEmitter,
    transform,
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

    for target in &targets {
        let emitter = target.emitter();
        let output = transform(&schema, emitter.as_ref(), &name, &args.namespace, args.schema_version)
            .with_context(|| format!("conversion failed for target {target:?}"))?;

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
