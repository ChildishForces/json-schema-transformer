#!/usr/bin/env bun
/**
 * Kotlin conformance harness: generate Runner.kt from the manifest, compile
 * ALL generated fixtures + runner in ONE kotlinc invocation, run the whole
 * suite in one JVM, write conformance/results/kotlin.json.
 *
 *   bun conformance/kotlin/run.ts
 *
 * Files that fail to compile are excluded iteratively (their groups are
 * marked all-tests-failed with the compiler error as the reason).
 */
import { $ } from 'bun';
import {
  existsSync,
  mkdirSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'fs';
import { dirname, join, resolve } from 'path';

const HERE = import.meta.dir;
const ROOT = resolve(HERE, '../..');
const GEN_DIR = join(ROOT, 'conformance/generated/kotlin');
const MANIFEST = join(ROOT, 'conformance/generated/manifest.json');
const BUILD = join(HERE, 'build');
const CLASSES = join(BUILD, 'classes');
const LIBS = join(HERE, 'libs');
const RESULTS_DIR = join(ROOT, 'conformance/results');
const RESULTS = join(RESULTS_DIR, 'kotlin.json');
const EXCLUSIONS = join(BUILD, 'exclusions.json');
const MAX_ATTEMPTS = 6;

// kotlinc's launcher defaults to -Xmx256M, far too small now that interpreter
// fixtures embed schemas + a mini-validator per file.
process.env.JAVA_OPTS ??= '-Xmx4g';

let kotlinc = process.env.KOTLINC ?? 'kotlinc';
let kotlinHome = process.env.KOTLIN_HOME;
if (!kotlinHome) {
  if (existsSync('/opt/homebrew/opt/kotlin/libexec/lib')) {
    kotlinHome = '/opt/homebrew/opt/kotlin/libexec';
  } else if (existsSync(join(HERE, 'toolchain/kotlinc/lib'))) {
    kotlinHome = join(HERE, 'toolchain/kotlinc');
    kotlinc = join(kotlinHome, 'bin/kotlinc');
  } else {
    const found = Bun.which(kotlinc);
    if (!found) {
      console.error(`kotlinc not found on PATH: ${kotlinc}`);
      process.exit(1);
    }
    kotlinHome = dirname(dirname(realpathSync(found)));
  }
}

const SER_PLUGIN = join(kotlinHome, 'lib/kotlinx-serialization-compiler-plugin.jar');
const STDLIB = join(kotlinHome, 'lib/kotlin-stdlib.jar');
if (!existsSync(SER_PLUGIN)) {
  console.error(`serialization compiler plugin not found: ${SER_PLUGIN}`);
  process.exit(1);
}

function libJar(stem: string): string {
  const hit = readdirSync(LIBS).find((f) => f.startsWith(stem) && f.endsWith('.jar'));
  if (!hit) {
    console.error(`${stem}*.jar not found in ${LIBS}`);
    process.exit(1);
  }
  return join(LIBS, hit);
}
const SER_JARS = [
  libJar('kotlinx-serialization-core-jvm-'),
  libJar('kotlinx-serialization-json-jvm-'),
].join(':');

interface Exclusions {
  files: Record<string, string>;
}

/** Parse kotlinc errors and add offending generated files to the exclusions. */
function addExclusions(log: string, exclusions: Exclusions): number {
  let added = 0;
  // kotlinc may print paths relative to its cwd, so match on the
  // generated/kotlin suffix and reduce to basenames.
  const re = /generated\/kotlin\/([A-Za-z0-9_]+\.kt):\d+:\d+: error: (.*)/g;
  for (const m of log.matchAll(re)) {
    const [, base, message] = m;
    if (!base || !message) continue;
    if (exclusions.files[base] === undefined) {
      const reason = `compile error: ${message.trim().slice(0, 200)}`;
      exclusions.files[base] = reason;
      console.log(`  excluding ${base} (${reason})`);
      added++;
    }
  }
  return added;
}

function errorSummary(log: string): string[] {
  const counts = new Map<string, number>();
  for (const line of log.split('\n')) {
    const at = line.indexOf('error:');
    if (at === -1) continue;
    const msg = line.slice(at);
    counts.set(msg, (counts.get(msg) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 10)
    .map(([msg, n]) => `  ${n}× ${msg}`);
}

rmSync(CLASSES, { recursive: true, force: true });
mkdirSync(CLASSES, { recursive: true });
mkdirSync(RESULTS_DIR, { recursive: true });

const exclusions: Exclusions = { files: {} };
const compileStart = Date.now();
let compiled = false;
let lastLog = '';

for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
  // 1. (Re)generate Runner.kt honouring current exclusions
  writeFileSync(EXCLUSIONS, JSON.stringify(exclusions, null, 2) + '\n');
  const gen = await $`bun ${join(HERE, 'gen-runner.ts')} ${MANIFEST} ${EXCLUSIONS} ${join(BUILD, 'Runner.kt')}`.nothrow();
  if (gen.exitCode !== 0) process.exit(gen.exitCode);

  // 2. Collect sources minus excluded files
  const sources = readdirSync(GEN_DIR)
    .filter((f) => f.endsWith('.kt') && exclusions.files[f] === undefined)
    .map((f) => join(GEN_DIR, f));
  sources.push(join(BUILD, 'Runner.kt'));
  const srcList = join(BUILD, 'sources.txt');
  writeFileSync(srcList, sources.join('\n') + '\n');

  console.log(`[attempt ${attempt}] compiling ${sources.length} files...`);
  const result = await $`${kotlinc} -nowarn -Xplugin=${SER_PLUGIN} -Xbackend-threads=0 -cp ${SER_JARS} -d ${CLASSES} @${srcList}`
    .quiet()
    .nothrow();
  lastLog = result.stdout.toString() + result.stderr.toString();
  writeFileSync(join(BUILD, 'kotlinc.log'), lastLog);

  if (result.exitCode === 0) {
    compiled = true;
    break;
  }

  // 3. Parse errors, exclude offending generated files
  console.log(`[attempt ${attempt}] compile failed; error summary:`);
  for (const line of errorSummary(lastLog)) console.log(line);

  if (addExclusions(lastLog, exclusions) === 0) {
    console.error(
      'compile errors are not attributable to generated files (Runner.kt or toolchain problem):'
    );
    console.error(lastLog.split('\n').slice(0, 40).join('\n'));
    process.exit(1);
  }
}

if (!compiled) {
  console.error(`compilation still failing after ${MAX_ATTEMPTS} attempts`);
  process.exit(1);
}

const compileTime = ((Date.now() - compileStart) / 1000).toFixed(0);
console.log(`compile OK in ${compileTime}s (excluded files: ${Object.keys(exclusions.files).length})`);

// 4. Run the whole suite in one JVM
const runStart = Date.now();
const run = await $`java -cp ${[CLASSES, STDLIB, SER_JARS].join(':')} RunnerKt ${MANIFEST} ${RESULTS}`.nothrow();
console.log(
  `compile time: ${compileTime}s, run time: ${((Date.now() - runStart) / 1000).toFixed(0)}s`
);
console.log(`results written to ${RESULTS}`);
process.exit(run.exitCode);
