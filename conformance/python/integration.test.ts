import { beforeAll, describe, expect, test } from 'bun:test';
import { copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

// Integration test: jst CLI → generated Pydantic module → runtime validation
// inside the harness venv. Covers title-based naming and shared-helpers mode.
// The validation script lives in check.template.py.
import { $ } from 'bun';

const ROOT = new URL('../..', import.meta.url).pathname.replace(/\/$/, '');
const JST = join(ROOT, 'target/debug/jst');
const TMP = join(import.meta.dir, '.itest');
const VENV_PY = join(import.meta.dir, '.venv/bin/python');

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

beforeAll(async () => {
  const build = await $`cargo build -p jst-cli`.cwd(ROOT).quiet().nothrow();
  if (build.exitCode !== 0) throw new Error(`cargo build failed: ${build.stderr.toString()}`);
  rmSync(TMP, { recursive: true, force: true });
  mkdirSync(TMP, { recursive: true });
  writeFileSync(join(TMP, 'sample.json'), JSON.stringify(SCHEMA));
  copyFileSync(join(import.meta.dir, 'check.template.py'), join(TMP, 'check.py'));
});

describe('pydantic integration', () => {
  test('collection mode validates correctly in the venv', async () => {
    if (!existsSync(VENV_PY)) {
      throw new Error(
        `venv missing — create it: python3 -m venv ${join(import.meta.dir, '.venv')} && .venv/bin/pip install pydantic regex`
      );
    }
    const gen = await $`${JST} sample.json --target pydantic -d .`.cwd(TMP).quiet().nothrow();
    if (gen.exitCode !== 0) throw new Error(`jst failed: ${gen.stderr.toString()}`);

    const result = await $`${VENV_PY} ${join(TMP, 'check.py')} ${join(TMP, 'order_item.py')}`
      .quiet()
      .nothrow();
    const out = result.stdout.toString();
    if (result.exitCode !== 0) {
      throw new Error(`python failed: ${result.stderr.toString()}\n${out}`);
    }
    expect(out.trim().split('\n').pop()).toBe('PASS');
  });
});
