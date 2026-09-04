// Integration test: jst CLI → generated Zod module → runtime validation.
// Covers title-based naming, shared-helpers mode, and inline mode.
import { $ } from 'bun';
import { beforeAll, describe, expect, test } from 'bun:test';
import { mkdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

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

const CASES: Array<[unknown, boolean]> = [
  [{ id: 'a', quantity: 2, tags: ['x', 'y'] }, true],
  [{ id: 'a', quantity: 1 }, true],
  [{ id: 'a', quantity: 0 }, false], // minimum
  [{ id: 'a', quantity: 1, tags: ['x', 'x'] }, false], // uniqueItems
  [{ id: 'a', quantity: 1, unknown: true }, false], // additionalProperties
  [{ quantity: 1 }, false], // required
];

async function runJst(args: string[]): Promise<void> {
  const result = await $`${JST} ${args}`.cwd(TMP).quiet().nothrow();
  if (result.exitCode !== 0) {
    throw new Error(`jst ${args.join(' ')} failed: ${result.stderr.toString()}`);
  }
}

beforeAll(async () => {
  const build = await $`cargo build -p jst-cli`.cwd(ROOT).quiet().nothrow();
  if (build.exitCode !== 0) throw new Error(`cargo build failed: ${build.stderr.toString()}`);
  rmSync(TMP, { recursive: true, force: true });
  mkdirSync(TMP, { recursive: true });
  writeFileSync(join(TMP, 'sample.json'), JSON.stringify(SCHEMA));
});

interface ZodLike {
  safeParse: (v: unknown) => { success: boolean };
}

describe('zod integration', () => {
  test('shared-helpers mode validates correctly', async () => {
    await runJst(['sample.json', '--target', 'zod', '-d', '.', '--helpers-file']);
    const mod = (await import(join(TMP, 'Order Item.zod.ts'))) as Record<string, ZodLike>;
    const schema = mod['OrderItemSchema'];
    expect(schema).toBeDefined();
    for (const [payload, expected] of CASES) {
      expect(schema!.safeParse(payload).success).toBe(expected);
    }
  });

  test('inline mode is self-contained and equivalent', async () => {
    await runJst(['sample.json', '--target', 'zod', '--out', 'inline.zod.ts']);
    const mod = (await import(join(TMP, 'inline.zod.ts'))) as Record<string, ZodLike>;
    const schema = mod['OrderItemSchema'];
    expect(schema).toBeDefined();
    for (const [payload, expected] of CASES) {
      expect(schema!.safeParse(payload).success).toBe(expected);
    }
  });

  test('name falls back to file stem when no title', async () => {
    writeFileSync(join(TMP, 'untitled-thing.json'), JSON.stringify({ type: 'string' }));
    await runJst(['untitled-thing.json', '--target', 'zod', '--out', 'stem.zod.ts']);
    const text = await Bun.file(join(TMP, 'stem.zod.ts')).text();
    expect(text).toContain('export const UntitledThingSchema');
  });
});
