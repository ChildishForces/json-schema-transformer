import { beforeAll, describe, expect, test } from 'bun:test';
import { copyFileSync, mkdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

// Integration test: jst CLI → generated Rust serde types + jst_helpers →
// compile with cargo → run decode assertions. Covers title-based naming and
// collection mode. The checker program lives in integration-main.template.rs.
import { $ } from 'bun';

const ROOT = new URL('../..', import.meta.url).pathname.replace(/\/$/, '');
const JST = join(ROOT, 'target/debug/jst');
const TMP = join(import.meta.dir, '.itest');

const SCHEMA = {
  title: 'Order Item',
  type: 'object',
  properties: {
    id: { type: 'string' },
    quantity: { type: 'integer', minimum: 1 },
    tags: { type: 'array', items: { type: 'string' }, uniqueItems: true },
  },
  required: ['id', 'quantity'],
  additionalProperties: false,
};

const CARGO_TOML = `[package]
name = "jst-integration-check"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
regex = "1"
`;

beforeAll(async () => {
  const build = await $`cargo build -p jst-cli`.cwd(ROOT).quiet().nothrow();
  if (build.exitCode !== 0) throw new Error(`cargo build failed: ${build.stderr.toString()}`);
  rmSync(TMP, { recursive: true, force: true });
  mkdirSync(join(TMP, 'src'), { recursive: true });
  writeFileSync(join(TMP, 'sample.json'), JSON.stringify(SCHEMA));
  writeFileSync(join(TMP, 'Cargo.toml'), CARGO_TOML);
  copyFileSync(join(import.meta.dir, 'integration-main.template.rs'), join(TMP, 'src/main.rs'));
});

describe('rust integration', () => {
  test('collection mode compiles and validates', async () => {
    const gen = await $`${JST} sample.json --target rust -d src`.cwd(TMP).quiet().nothrow();
    if (gen.exitCode !== 0) throw new Error(`jst failed: ${gen.stderr.toString()}`);

    const compile = await $`cargo build`.cwd(TMP).quiet().nothrow();
    if (compile.exitCode !== 0) throw new Error(`cargo failed: ${compile.stderr.toString()}`);

    const run = await $`${join(TMP, 'target/debug/jst-integration-check')}`.quiet().nothrow();
    const out = run.stdout.toString();
    if (run.exitCode !== 0) throw new Error(`runner failed: ${run.stderr.toString()}\n${out}`);
    expect(out.trim().split('\n').pop()).toBe('PASS');
  }, 180_000);
});
