import { beforeAll, describe, expect, test } from 'bun:test';
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'fs';
import { join } from 'path';

// Integration test: jst CLI → generated Kotlin @Serializable + JstHelpers →
// compile with kotlinc + serialization plugin → run decode assertions.
// Covers title-based naming and collection mode. The checker program
// lives in IntegrationMain.template.kt.
import { $ } from 'bun';

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
});

/** Generate into `dir`, compile the generated files + Main.kt, run MainKt. */
async function generateCompileRun(
  dir: string,
  template: string,
  jstFlags: string[]
): Promise<string> {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'sample.json'), JSON.stringify(SCHEMA));
  copyFileSync(join(HERE, template), join(dir, 'Main.kt'));

  const gen = await $`${JST} sample.json --target kotlin ${jstFlags} -d .`
    .cwd(dir)
    .quiet()
    .nothrow();
  if (gen.exitCode !== 0) throw new Error(`jst failed: ${gen.stderr.toString()}`);

  const home = kotlinHome();
  const plugin = join(home, 'lib/kotlinx-serialization-compiler-plugin.jar');
  const stdlib = join(home, 'lib/kotlin-stdlib.jar');
  const jars = serJars();
  const classpath = jars.join(':');

  const compile =
    await $`kotlinc -classpath ${classpath} ${`-Xplugin=${plugin}`} OrderItem.kt JstHelpers.kt Main.kt -d classes -nowarn`
      .cwd(dir)
      .env({ ...process.env, JAVA_OPTS: '-Xmx4g' })
      .quiet()
      .nothrow();
  if (compile.exitCode !== 0) throw new Error(`kotlinc failed: ${compile.stderr.toString()}`);

  const run = await $`java -cp ${['classes', stdlib, ...jars].join(':')} MainKt`
    .cwd(dir)
    .quiet()
    .nothrow();
  const out = run.stdout.toString();
  if (run.exitCode !== 0) throw new Error(`java failed: ${run.stderr.toString()}\n${out}`);
  return out;
}

describe('kotlin integration', () => {
  test('collection mode compiles and validates', async () => {
    const out = await generateCompileRun(TMP, 'IntegrationMain.template.kt', []);
    expect(out.trim().split('\n').pop()).toBe('PASS');
  }, 240_000);

  test('--mutable emits var properties and validate() catches mutations', async () => {
    const out = await generateCompileRun(join(TMP, 'mutable'), 'IntegrationMutation.template.kt', [
      '--mutable',
    ]);
    expect(out.trim().split('\n').pop()).toBe('PASS');
  }, 240_000);
});
