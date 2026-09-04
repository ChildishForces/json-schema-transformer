// Integration test: jst CLI → generated Swift Codable + JstHelpers → compile
// with swiftc → run decode assertions. Covers title-based naming and
// shared-helpers mode. The checker program lives in
// integration-main.template.swift.
import { $ } from 'bun';
import { beforeAll, describe, expect, test } from 'bun:test';
import { copyFileSync, mkdirSync, rmSync, writeFileSync } from 'fs';
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

beforeAll(async () => {
  const build = await $`cargo build -p jst-cli`.cwd(ROOT).quiet().nothrow();
  if (build.exitCode !== 0) throw new Error(`cargo build failed: ${build.stderr.toString()}`);
  rmSync(TMP, { recursive: true, force: true });
  mkdirSync(TMP, { recursive: true });
  writeFileSync(join(TMP, 'sample.json'), JSON.stringify(SCHEMA));
  copyFileSync(join(import.meta.dir, 'integration-main.template.swift'), join(TMP, 'main.swift'));
});

describe('swift integration', () => {
  test(
    'shared-helpers mode compiles and validates',
    async () => {
      const gen = await $`${JST} sample.json --target swift -d . --helpers-file`
        .cwd(TMP)
        .quiet()
        .nothrow();
      if (gen.exitCode !== 0) throw new Error(`jst failed: ${gen.stderr.toString()}`);

      const compile =
        await $`swiftc -Onone -suppress-warnings ${'Order Item.swift'} JstHelpers.swift main.swift -o runner`
          .cwd(TMP)
          .quiet()
          .nothrow();
      if (compile.exitCode !== 0) throw new Error(`swiftc failed: ${compile.stderr.toString()}`);

      const run = await $`${join(TMP, 'runner')}`.quiet().nothrow();
      const out = run.stdout.toString();
      if (run.exitCode !== 0) throw new Error(`runner failed: ${run.stderr.toString()}\n${out}`);
      expect(out.trim().split('\n').pop()).toBe('PASS');
    },
    120_000
  );
});
