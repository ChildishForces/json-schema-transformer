#!/usr/bin/env bun
import { mkdirSync, readFileSync, writeFileSync } from 'fs';
import { cpus } from 'os';
import { basename, join, resolve } from 'path';

/**
 * Swift conformance harness: compile ALL generated fixtures into one binary,
 * run the whole suite in one process.
 *
 *   bun conformance/swift/run.ts
 *
 * The runner's main.swift is produced from main.template.swift by replacing
 * the `// @jst:registrations` marker with the type_name → decode-closure
 * table. Files that fail to compile are excluded iteratively (their groups
 * are marked all-tests-failed with the compiler error as the reason).
 */
import { $ } from 'bun';

const HERE = import.meta.dir;
const GEN_DIR = resolve(HERE, '../generated');
const BUILD = join(HERE, '.build');
const RESULTS = resolve(HERE, '../results/swift.json');
const MANIFEST = join(GEN_DIR, 'manifest.json');
const EXCLUSIONS = join(BUILD, 'exclusions.json');
const TEMPLATE = join(HERE, 'main.template.swift');
const MAX_ATTEMPTS = 6;

interface Group {
  id: string;
  keyword: string;
  type_name: string;
  files?: Record<string, string>;
  errors?: Record<string, string>;
}

type Exclusions = Record<string, string>;

/** Build main.swift from the template and the sources list, minus exclusions. */
function generate(exclusions: Exclusions): { sources: string[]; decoders: number } {
  const manifest = JSON.parse(readFileSync(MANIFEST, 'utf8')) as { groups: Group[] };

  const sources: string[] = [];
  const registrations: string[] = [];
  for (const g of manifest.groups) {
    const file = g.files?.swift;
    if (!file || g.errors?.swift) continue;
    if (exclusions[basename(file)] !== undefined) continue;
    sources.push(resolve(GEN_DIR, file));
    registrations.push(
      `    m["${g.type_name}"] = { d in _ = try JSONDecoder().decode(${g.type_name}.self, from: d) }`
    );
  }

  // Chunk registrations into small functions to keep type-checking fast.
  const CHUNK = 40;
  const chunks: string[] = [];
  for (let i = 0; i * CHUNK < registrations.length; i++) {
    const body = registrations.slice(i * CHUNK, (i + 1) * CHUNK).join('\n');
    chunks.push(`func register${i}(_ m: inout [String: (Data) throws -> Void]) {\n${body}\n}`);
  }
  const registerAll = chunks.map((_, i) => `    register${i}(&m)`).join('\n');
  const table = `${chunks.join('\n\n')}\n\nfunc registerAll(_ m: inout [String: (Data) throws -> Void]) {\n${registerAll}\n}`;

  const template = readFileSync(TEMPLATE, 'utf8');
  const marker = '// @jst:registrations';
  if (!template.includes(marker)) throw new Error(`marker ${marker} missing from template`);
  writeFileSync(join(BUILD, 'main.swift'), template.replace(marker, table));
  writeFileSync(join(BUILD, 'sources.txt'), sources.join('\n') + '\n');
  return { sources, decoders: registrations.length };
}

/** Parse swiftc errors and add offending generated files to the exclusions. */
function addExclusions(log: string, exclusions: Exclusions): number {
  let added = 0;
  const re = /^([^\s:][^:]*\/generated\/swift\/[^:]+\.swift):\d+:\d+: error: (.*)$/gm;
  for (const m of log.matchAll(re)) {
    const [, path, message] = m;
    if (!path || !message) continue;
    const file = basename(path);
    if (exclusions[file] === undefined) {
      exclusions[file] = message.trim();
      added++;
    }
  }
  return added;
}

mkdirSync(BUILD, { recursive: true });
mkdirSync(resolve(HERE, '../results'), { recursive: true });

const exclusions: Exclusions = {};
const ncpu = cpus().length;

const compileStart = Date.now();

let compiled = false;
let lastLog = '';

for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
  const { sources } = generate(exclusions);
  console.log(`compile attempt ${attempt} (${sources.length} fixture files)...`);

  const result =
    await $`swiftc -Onone -suppress-warnings -j ${ncpu} -module-name ConformanceRunner -o ${join(BUILD, 'runner')} @${join(BUILD, 'sources.txt')} ${join(GEN_DIR, 'swift/JstHelpers.swift')} ${join(BUILD, 'main.swift')}`
      .quiet()
      .nothrow();
  lastLog = result.stderr.toString();

  if (result.exitCode === 0) {
    compiled = true;
    break;
  }

  const added = addExclusions(lastLog, exclusions);
  console.log(`  compile failed; excluded ${added} file(s)`);

  if (added === 0) {
    console.error('compile failed with no attributable generated file:');
    console.error(lastLog.split('\n').slice(-40).join('\n'));
    process.exit(1);
  }
}

if (!compiled) {
  console.error('still failing after exclusion attempts:');
  console.error(lastLog.split('\n').slice(-40).join('\n'));
  process.exit(1);
}

writeFileSync(EXCLUSIONS, JSON.stringify(exclusions, null, 2) + '\n');

const compileTime = ((Date.now() - compileStart) / 1000).toFixed(0);
console.log(`compiled in ${compileTime}s (excluded ${Object.keys(exclusions).length} file(s))`);

const runStart = Date.now();
await $`${join(BUILD, 'runner')} ${MANIFEST} ${RESULTS} ${EXCLUSIONS}`;

console.log(
  `total run time: ${((Date.now() - runStart) / 1000).toFixed(0)}s (compile ${compileTime}s)`
);
