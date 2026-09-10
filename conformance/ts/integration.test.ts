import { beforeAll, describe, expect, test } from 'bun:test';
import { existsSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from 'fs';
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

async function runJstExpectingError(args: string[]): Promise<string> {
  const result = await $`${JST} ${args}`.cwd(TMP).quiet().nothrow();
  expect(result.exitCode).not.toBe(0);
  return result.stderr.toString();
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

  test('output collisions are caught pre-write, including the helpers file', async () => {
    mkdirSync(join(TMP, 'clash'), { recursive: true });
    writeFileSync(join(TMP, 'clash/a.json'), JSON.stringify({ title: 'FooBar', type: 'object' }));
    writeFileSync(join(TMP, 'clash/b.json'), JSON.stringify({ title: 'Foobar', type: 'object' }));
    // Stems differing only by case collide on case-insensitive filesystems.
    const caseErr = await runJstExpectingError(['clash', '--target', 'zod', '-d', 'clash-out']);
    expect(caseErr).toContain('output collision');
    expect(existsSync(join(TMP, 'clash-out'))).toBe(false); // pre-flight: nothing written

    // A schema whose stem matches the shared helpers file name must not clobber it.
    writeFileSync(
      join(TMP, 'helpers-clash.json'),
      JSON.stringify({
        title: 'Jst Helpers',
        type: 'array',
        items: { type: 'integer' },
        uniqueItems: true,
      })
    );
    const helpersErr = await runJstExpectingError([
      'helpers-clash.json',
      '--target',
      'pydantic',
      '-d',
      'hc-out',
    ]);
    expect(helpersErr).toContain('shared helpers file');
  });

  test('symlinked directories are not followed', async () => {
    mkdirSync(join(TMP, 'loop/sub'), { recursive: true });
    writeFileSync(join(TMP, 'loop/sub/one.json'), JSON.stringify(SCHEMA));
    symlinkSync(join(TMP, 'loop'), join(TMP, 'loop/sub/back'));
    await runJst(['loop', '--target', 'zod', '-d', 'loop-out']);
    expect(existsSync(join(TMP, 'loop-out/sub/OrderItem.zod.ts'))).toBe(true);
    expect(existsSync(join(TMP, 'loop-out/sub/back'))).toBe(false);
  });

  test('--helpers-file names are validated per target', async () => {
    const extErr = await runJstExpectingError([
      'sample.json',
      '--target',
      'zod',
      '-d',
      'hf-out',
      '--helpers-file',
      'utils',
    ]);
    expect(extErr).toContain('must end with .ts');
    const identErr = await runJstExpectingError([
      'sample.json',
      '--target',
      'pydantic',
      '-d',
      'hf-out',
      '--helpers-file',
      'my-helpers.py',
    ]);
    expect(identErr).toContain('not importable');
  });

  test('--name applies to a single-schema directory input', async () => {
    mkdirSync(join(TMP, 'onedir'), { recursive: true });
    writeFileSync(join(TMP, 'onedir/thing.json'), JSON.stringify(SCHEMA));
    await runJst(['onedir', '--target', 'zod', '-d', 'onedir-out', '--name', 'Override']);
    expect(existsSync(join(TMP, 'onedir-out/Override.zod.ts'))).toBe(true);
    // and errors with multiple schemas
    writeFileSync(
      join(TMP, 'onedir/thing2.json'),
      JSON.stringify({ title: 'Two', type: 'object' })
    );
    const err = await runJstExpectingError([
      'onedir',
      '--target',
      'zod',
      '-d',
      'onedir-out',
      '--name',
      'Override',
    ]);
    expect(err).toContain('ambiguous');
  });

  test('name falls back to file stem when no title', async () => {
    writeFileSync(join(TMP, 'untitled-thing.json'), JSON.stringify({ type: 'string' }));
    await runJst(['untitled-thing.json', '--target', 'zod', '--out', 'stem.zod.ts']);
    const text = await Bun.file(join(TMP, 'stem.zod.ts')).text();
    expect(text).toContain('export const UntitledThingSchema');
  });
});
