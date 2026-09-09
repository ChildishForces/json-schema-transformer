import { beforeAll, describe, expect, test } from 'bun:test';
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

// Integration test: jst CLI → generated Zod module → runtime validation.
// Covers title-based naming, collection mode, and single-file (inline) mode.
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
  test('collection mode validates correctly', async () => {
    await runJst(['sample.json', '--target', 'zod', '-d', '.']);
    const mod = (await import(join(TMP, 'OrderItem.zod.ts'))) as Record<string, ZodLike>;
    const schema = mod['OrderItemSchema'];
    expect(schema).toBeDefined();
    for (const [payload, expected] of CASES) {
      expect(schema!.safeParse(payload).success).toBe(expected);
    }
  });

  test('single-file mode is self-contained and equivalent', async () => {
    await runJst(['sample.json', '--target', 'zod', '--out', 'inline.zod.ts']);
    const mod = (await import(join(TMP, 'inline.zod.ts'))) as Record<string, ZodLike>;
    const schema = mod['OrderItemSchema'];
    expect(schema).toBeDefined();
    for (const [payload, expected] of CASES) {
      expect(schema!.safeParse(payload).success).toBe(expected);
    }
  });

  test('collection mode mirrors directory structure with depth-aware imports', async () => {
    mkdirSync(join(TMP, 'schemas/orders'), { recursive: true });
    mkdirSync(join(TMP, 'schemas/users'), { recursive: true });
    writeFileSync(join(TMP, 'schemas/orders/item.json'), JSON.stringify(SCHEMA));
    writeFileSync(
      join(TMP, 'schemas/users/profile.json'),
      JSON.stringify({
        title: 'User Profile',
        type: 'object',
        properties: { name: { type: 'string' } },
      })
    );
    await runJst(['schemas', '--target', 'zod', '-d', 'gen']);

    // Mirrored tree: schemas/orders/item.json → gen/orders/OrderItem.zod.ts
    const nestedPath = join(TMP, 'gen/orders/OrderItem.zod.ts');
    expect(existsSync(nestedPath)).toBe(true);
    expect(existsSync(join(TMP, 'gen/users/UserProfile.zod.ts'))).toBe(true);

    // Nested modules import the root helpers file with a depth-aware path
    const nested = await Bun.file(nestedPath).text();
    expect(nested).toContain('from "../jst-helpers"');
    expect(existsSync(join(TMP, 'gen/jst-helpers.ts'))).toBe(true);

    // And the nested module still validates end-to-end
    const mod = (await import(nestedPath)) as Record<string, ZodLike>;
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
