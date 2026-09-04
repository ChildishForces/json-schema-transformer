// Integration test: jst CLI → generated Kotlin @Serializable + JstHelpers →
// compile with kotlinc + serialization plugin → run decode assertions.
// Covers title-based naming and shared-helpers mode. The checker program
// lives in IntegrationMain.template.kt.
import { $ } from 'bun';
import { beforeAll, describe, expect, test } from 'bun:test';
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

const ROOT = new URL('../..', import.meta.url).pathname.replace(/\/$/, '');
const JST = join(ROOT, 'target/debug/jst');
const HERE = import.meta.dir;
const TMP = join(HERE, '.itest');
const LIBS = join(HERE, 'libs');

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

/** Mirror run.sh's toolchain resolution. */
function kotlinHome(): string {
  if (process.env.KOTLIN_HOME) return process.env.KOTLIN_HOME;
  if (existsSync('/opt/homebrew/opt/kotlin/libexec/lib')) return '/opt/homebrew/opt/kotlin/libexec';
  if (existsSync(join(HERE, 'toolchain/kotlinc/lib'))) return join(HERE, 'toolchain/kotlinc');
  throw new Error('cannot locate KOTLIN_HOME');
}

function serJars(): string[] {
  return readdirSync(LIBS)
    .filter((f) => f.startsWith('kotlinx-serialization-') && f.endsWith('.jar'))
    .map((f) => join(LIBS, f));
}

beforeAll(async () => {
  const build = await $`cargo build -p jst-cli`.cwd(ROOT).quiet().nothrow();
  if (build.exitCode !== 0) throw new Error(`cargo build failed: ${build.stderr.toString()}`);
  rmSync(TMP, { recursive: true, force: true });
  mkdirSync(TMP, { recursive: true });
  writeFileSync(join(TMP, 'sample.json'), JSON.stringify(SCHEMA));
  copyFileSync(join(HERE, 'IntegrationMain.template.kt'), join(TMP, 'Main.kt'));
});

describe('kotlin integration', () => {
  test(
    'shared-helpers mode compiles and validates',
    async () => {
      const gen = await $`${JST} sample.json --target kotlin -d . --helpers-file`
        .cwd(TMP)
        .quiet()
        .nothrow();
      if (gen.exitCode !== 0) throw new Error(`jst failed: ${gen.stderr.toString()}`);

      const home = kotlinHome();
      const plugin = join(home, 'lib/kotlinx-serialization-compiler-plugin.jar');
      const stdlib = join(home, 'lib/kotlin-stdlib.jar');
      const jars = serJars();
      const classpath = jars.join(':');

      const compile =
        await $`kotlinc -classpath ${classpath} ${`-Xplugin=${plugin}`} ${'Order Item.kt'} JstHelpers.kt Main.kt -d classes -nowarn`
          .cwd(TMP)
          .env({ ...process.env, JAVA_OPTS: '-Xmx4g' })
          .quiet()
          .nothrow();
      if (compile.exitCode !== 0) throw new Error(`kotlinc failed: ${compile.stderr.toString()}`);

      const run = await $`java -cp ${['classes', stdlib, ...jars].join(':')} MainKt`
        .cwd(TMP)
        .quiet()
        .nothrow();
      const out = run.stdout.toString();
      if (run.exitCode !== 0) throw new Error(`java failed: ${run.stderr.toString()}\n${out}`);
      expect(out.trim().split('\n').pop()).toBe('PASS');
    },
    240_000
  );
});
