#!/usr/bin/env bun
// Bumps the workspace version to the given semver:
//   bun run bump 0.2.0
//
// Updates [workspace.package] in the root Cargo.toml (inherited by all
// crates), the pinned dep version in jst-cli's manifest, and Cargo.lock.
import { $ } from 'bun';
import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

const ROOT = join(import.meta.dir, '..');
const SEMVER =
  /^\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

const version = process.argv[2];
if (!version || !SEMVER.test(version)) {
  console.error('usage: bun run bump <semver>   (e.g. bun run bump 0.2.0)');
  process.exit(2);
}

const edits: Array<[rel: string, re: RegExp]> = [
  ['Cargo.toml', /(\[workspace\.package\]\s*\nversion = ")[^"]+(")/],
  ['crates/jst-cli/Cargo.toml', /(json-schema-transformer = \{[^}]*version = ")[^"]+(")/],
];

for (const [rel, re] of edits) {
  const path = join(ROOT, rel);
  const src = readFileSync(path, 'utf8');
  const out = src.replace(re, (_, before: string, after: string) => before + version + after);
  if (out === src && !src.includes(`"${version}"`)) {
    console.error(`no version field matched in ${rel}`);
    process.exit(1);
  }
  writeFileSync(path, out);
  console.log(`${rel} → ${version}`);
}

const lock = await $`cargo update --workspace --quiet`.cwd(ROOT).nothrow();
if (lock.exitCode !== 0) process.exit(lock.exitCode);
console.log(`Cargo.lock → ${version}`);
console.log(`\nrelease with: git tag v${version} && git push --tags`);
