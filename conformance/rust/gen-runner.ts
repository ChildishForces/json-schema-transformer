#!/usr/bin/env bun
// Generates src/main.rs for the Rust conformance harness from
// main.template.rs, replacing the `// @jst:mods`, `// @jst:registrations`
// and `// @jst:excluded` markers.
//
// Usage: bun gen-runner.ts <manifest.json> <exclusions.json> <out-main.rs>
//
// exclusions.json: { "files": { "ref_g35.rs": "reason string", ... } }
// Groups whose generated file is excluded get no mod/decoder entry; the
// runner marks all of their tests as failed with the recorded reason.

import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

interface ManifestGroup {
  id: string;
  keyword: string;
  type_name: string;
  files?: Record<string, string>;
  errors?: Record<string, string>;
}

interface Manifest {
  groups: ManifestGroup[];
}

interface Exclusions {
  files?: Record<string, string>;
}

const [manifestPath, exclusionsPath, outPath] = process.argv.slice(2);
if (!manifestPath || !exclusionsPath || !outPath) {
  console.error('usage: bun gen-runner.ts <manifest.json> <exclusions.json> <out-main.rs>');
  process.exit(2);
}

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as Manifest;
const exclusions = JSON.parse(readFileSync(exclusionsPath, 'utf8')) as Exclusions;
const excludedFiles: Record<string, string> = exclusions.files ?? {};

const rustEscape = (s: string): string =>
  s
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t');

const mods: string[] = ['mod jst_helpers;'];
const registrations: string[] = [];
const excludedEntries: Array<[string, string]> = [];
const seen = new Set<string>();

for (const g of manifest.groups) {
  const typeName = g.type_name;
  if (seen.has(typeName)) continue;
  seen.add(typeName);
  const rsFile = g.files?.rust; // e.g. "rust/ref_g35.rs"
  const genError = g.errors?.rust;
  const base = rsFile?.split('/').pop();
  if (genError || !rsFile || !base) {
    excludedEntries.push([typeName, `generation error: ${genError ?? 'no rust file'}`]);
  } else if (base in excludedFiles) {
    excludedEntries.push([typeName, `excluded from compilation: ${excludedFiles[base]}`]);
  } else {
    const modName = base.replace(/\.rs$/, '');
    mods.push(`mod ${modName};`);
    registrations.push(
      `    m.insert("${typeName}", (|s| serde_json::from_str::<crate::${modName}::${typeName}>(s).map(|_| ()).map_err(|e| e.to_string())) as Decoder);`
    );
  }
}

const modsBlock = mods.join('\n');

const registrationsBlock =
  `fn decoders() -> std::collections::HashMap<&'static str, Decoder> {\n` +
  `    let mut m: std::collections::HashMap<&'static str, Decoder> = std::collections::HashMap::new();\n` +
  registrations.join('\n') +
  `\n    m\n}`;

const excludedBlock =
  `fn excluded() -> std::collections::HashMap<&'static str, String> {\n` +
  `    let mut m: std::collections::HashMap<&'static str, String> = std::collections::HashMap::new();\n` +
  excludedEntries
    .map(([t, r]) => `    m.insert("${rustEscape(t)}", "${rustEscape(r)}".to_string());`)
    .join('\n') +
  `\n    m\n}`;

const template = readFileSync(join(import.meta.dir, 'main.template.rs'), 'utf8');
for (const marker of ['// @jst:mods', '// @jst:registrations', '// @jst:excluded']) {
  if (!template.includes(marker)) {
    console.error(`marker ${marker} missing from main.template.rs`);
    process.exit(1);
  }
}

const out = template
  .replace('// @jst:mods', modsBlock)
  .replace('// @jst:registrations', registrationsBlock)
  .replace('// @jst:excluded', excludedBlock);

writeFileSync(outPath, out);
console.error(
  `main.rs: ${registrations.length} decoders, ${excludedEntries.length} excluded groups`
);
