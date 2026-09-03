// Zod conformance harness.
//
// Loads conformance/generated/manifest.json, dynamically imports each group's
// generated Zod module, runs safeParse on every test case, and compares the
// result to the expected validity. Groups with a generation error or a module
// that fails to import count ALL of their tests as failures (no skips).
//
// Run with: bun run conformance/ts/run.ts

import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

interface TestCase {
  description: string;
  data: unknown;
  valid: boolean;
}

interface Group {
  id: string;
  keyword: string;
  group_index: number;
  description: string;
  type_name: string;
  schema: unknown;
  files: Record<string, string>;
  errors: Record<string, string>;
  tests: TestCase[];
}

interface Failure {
  id: string;
  keyword: string;
  test: string;
  expected: boolean;
  actual: boolean | null;
  reason?: string;
}

interface Results {
  language: "zod";
  total: number;
  pass: number;
  fail: number;
  failures: Failure[];
  byKeyword: Record<string, { total: number; pass: number }>;
}

const conformanceDir = resolve(import.meta.dir, "..");
const manifestPath = join(conformanceDir, "generated", "manifest.json");
const resultsPath = join(conformanceDir, "results", "zod.json");

const manifest = (await Bun.file(manifestPath).json()) as { groups: Group[] };

const results: Results = {
  language: "zod",
  total: 0,
  pass: 0,
  fail: 0,
  failures: [],
  byKeyword: {},
};

function record(group: Group, test: TestCase, pass: boolean, actual: boolean | null, reason?: string) {
  results.total++;
  const kw = (results.byKeyword[group.keyword] ??= { total: 0, pass: 0 });
  kw.total++;
  if (pass) {
    results.pass++;
    kw.pass++;
  } else {
    results.fail++;
    results.failures.push({
      id: group.id,
      keyword: group.keyword,
      test: test.description,
      expected: test.valid,
      actual,
      ...(reason ? { reason } : {}),
    });
  }
}

function failGroup(group: Group, reason: string) {
  for (const test of group.tests) record(group, test, false, null, reason);
}

const start = performance.now();

for (const group of manifest.groups) {
  if (group.errors?.zod) {
    failGroup(group, `generation error: ${group.errors.zod}`);
    continue;
  }
  const rel = group.files?.zod;
  if (!rel) {
    failGroup(group, "no zod module generated");
    continue;
  }

  let mod: Record<string, unknown>;
  try {
    mod = await import(join(conformanceDir, "generated", rel));
  } catch (e) {
    failGroup(group, `import error: ${e instanceof Error ? e.message : String(e)}`);
    continue;
  }

  const exportName = `${group.type_name}Schema`;
  const schema = mod[exportName] as { safeParse(data: unknown): { success: boolean } } | undefined;
  if (!schema || typeof schema.safeParse !== "function") {
    failGroup(group, `missing export ${exportName}`);
    continue;
  }

  for (const test of group.tests) {
    let actual: boolean;
    try {
      actual = schema.safeParse(test.data).success;
    } catch (e) {
      record(group, test, false, null, `runtime error: ${e instanceof Error ? e.message : String(e)}`);
      continue;
    }
    record(group, test, actual === test.valid, actual);
  }
}

const elapsed = performance.now() - start;

mkdirSync(join(conformanceDir, "results"), { recursive: true });
writeFileSync(resultsPath, JSON.stringify(results, null, 2) + "\n");

// --- Human summary ---
const pct = results.total > 0 ? (results.pass / results.total) * 100 : 0;
console.log(`zod conformance: ${results.pass}/${results.total} passed, ${results.fail} failed (${pct.toFixed(2)}%) in ${(elapsed / 1000).toFixed(2)}s`);
console.log(`results written to ${resultsPath}`);

const worst = Object.entries(results.byKeyword)
  .map(([keyword, { total, pass }]) => ({ keyword, total, pass, fail: total - pass }))
  .filter((k) => k.fail > 0)
  .sort((a, b) => b.fail - a.fail)
  .slice(0, 15);

if (worst.length > 0) {
  console.log("\nTop failing keywords:");
  for (const k of worst) {
    console.log(`  ${k.keyword.padEnd(24)} ${String(k.fail).padStart(4)} failed / ${k.total} (${((k.pass / k.total) * 100).toFixed(1)}% pass)`);
  }
} else {
  console.log("\nNo failing keywords.");
}

process.exit(0);
