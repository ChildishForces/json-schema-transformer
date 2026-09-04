#!/usr/bin/env bun
// Generates Runner.kt for the Kotlin conformance harness from
// Runner.template.kt, replacing the `// @jst:decoders` marker with the
// type_name → decode-lambda table and `// @jst:excluded` with the map of
// excluded groups.
//
// Usage: bun gen-runner.ts <manifest.json> <exclusions.json> <out-Runner.kt>
//
// exclusions.json: { "files": { "RefG35.kt": "reason string", ... } }
// Groups whose generated file is excluded get no decoder entry; the runner
// marks all of their tests as failed with the recorded reason.

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
  console.error('usage: bun gen-runner.ts <manifest.json> <exclusions.json> <out-Runner.kt>');
  process.exit(2);
}

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as Manifest;
const exclusions = JSON.parse(readFileSync(exclusionsPath, 'utf8')) as Exclusions;
const excludedFiles: Record<string, string> = exclusions.files ?? {};

const kotlinEscape = (s: string): string =>
  s
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\$/g, '\\$')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t');

const decoderEntries: string[] = [];
const excludedEntries: Array<[string, string]> = [];
const seen = new Set<string>();

for (const g of manifest.groups) {
  const typeName = g.type_name;
  if (seen.has(typeName)) continue;
  seen.add(typeName);
  const ktFile = g.files?.kotlin; // e.g. "kotlin/RefG35.kt"
  const genError = g.errors?.kotlin;
  const base = ktFile?.split('/').pop();
  if (genError || !ktFile) {
    excludedEntries.push([typeName, `generation error: ${genError ?? 'no kotlin file'}`]);
  } else if (base !== undefined && base in excludedFiles) {
    excludedEntries.push([typeName, `excluded from compilation: ${excludedFiles[base]}`]);
  } else {
    decoderEntries.push(typeName);
  }
}

// Chunk decoder map builders to keep any single method small.
const CHUNK = 50;
const chunks: string[][] = [];
for (let i = 0; i < decoderEntries.length; i += CHUNK) {
  chunks.push(decoderEntries.slice(i, i + CHUNK));
}

const decoderFns = chunks
  .map(
    (chunk, i) =>
      `private fun decoders${i}(): Map<String, (String) -> Unit> = mapOf(\n` +
      chunk.map((t) => `    "${t}" to { j -> strictJson.decodeFromString<${t}>(j) }`).join(',\n') +
      `\n)`
  )
  .join('\n\n');

const decodersBlock =
  `${decoderFns}\n\n` +
  `private val decoders: Map<String, (String) -> Unit> = buildMap {\n` +
  chunks.map((_, i) => `    putAll(decoders${i}())`).join('\n') +
  `\n}`;

const excludedBlock =
  `private val excluded: Map<String, String> = mapOf(\n` +
  excludedEntries.map(([t, r]) => `    "${kotlinEscape(t)}" to "${kotlinEscape(r)}"`).join(',\n') +
  `\n)`;

const template = readFileSync(join(import.meta.dir, 'Runner.template.kt'), 'utf8');
for (const marker of ['// @jst:decoders', '// @jst:excluded']) {
  if (!template.includes(marker)) {
    console.error(`marker ${marker} missing from Runner.template.kt`);
    process.exit(1);
  }
}

const out = template
  .replace('// @jst:decoders', decodersBlock)
  .replace('// @jst:excluded', excludedBlock);

writeFileSync(outPath, out);
console.error(
  `Runner.kt: ${decoderEntries.length} decoders, ${excludedEntries.length} excluded groups`
);
