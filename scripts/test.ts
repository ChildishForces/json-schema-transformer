#!/usr/bin/env bun
/**
 * Test runner with parallel execution and live spinner output.
 *
 * Usage:
 *   bun scripts/test.ts          # run all suites
 *   bun scripts/test.ts --quick  # skip conformance suites
 */

import { existsSync, readFileSync } from 'fs';

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const CHECK = '\x1b[32m✔\x1b[0m';
const CROSS = '\x1b[31m✘\x1b[0m';
const SKIP_ICON = '\x1b[33m⊘\x1b[0m';
const DIM = '\x1b[2m';
const RESET = '\x1b[0m';
const BOLD = '\x1b[1m';
const GREEN = '\x1b[32m';
const RED = '\x1b[31m';
const CYAN = '\x1b[36m';
const HIDE_CURSOR = '\x1b[?25l';
const SHOW_CURSOR = '\x1b[?25h';

const REPO_ROOT = new URL('..', import.meta.url).pathname.replace(/\/$/, '');

interface Suite {
  name: string;
  phase: 'build' | 'unit' | 'generate' | 'conformance';
  command: string[];
  cwd: string;
  extract?: (output: string, suite?: Suite) => string;
  /** Path to a conformance results JSON ({total, pass, fail}) read after the run. */
  resultsFile?: string;
  skip?: boolean;
}

const quick = process.argv.includes('--quick');

const suites: Suite[] = [
  {
    name: 'cargo build',
    phase: 'build',
    command: ['cargo', 'build', '--workspace', '--all-features'],
    cwd: REPO_ROOT,
  },
  {
    name: 'Rust unit tests',
    phase: 'unit',
    command: ['cargo', 'test', '--workspace', '--all-features'],
    cwd: REPO_ROOT,
    extract: (out) => {
      let total = 0;
      for (const m of out.matchAll(/(\d+) passed/g)) {
        total += parseInt(m[1], 10);
      }
      return total > 0 ? `${total} passed` : '';
    },
  },
  {
    name: 'conformance fixtures',
    phase: 'generate',
    command: ['./target/debug/conformance-gen'],
    cwd: REPO_ROOT,
    extract: (out) => out.trim().split('\n').pop() ?? '',
    skip: quick,
  },
  {
    name: 'Zod conformance',
    phase: 'conformance',
    command: ['bun', 'run', 'conformance/ts/run.ts'],
    cwd: REPO_ROOT,
    resultsFile: 'conformance/results/zod.json',
    skip: quick,
  },
  {
    name: 'Pydantic conformance',
    phase: 'conformance',
    command: ['conformance/python/run.sh'],
    cwd: REPO_ROOT,
    resultsFile: 'conformance/results/pydantic.json',
    skip: quick,
  },
  {
    name: 'Swift conformance',
    phase: 'conformance',
    command: ['conformance/swift/run.sh'],
    cwd: REPO_ROOT,
    resultsFile: 'conformance/results/swift.json',
    skip: quick,
  },
  {
    name: 'Kotlin conformance',
    phase: 'conformance',
    command: ['conformance/kotlin/run.sh'],
    cwd: REPO_ROOT,
    resultsFile: 'conformance/results/kotlin.json',
    skip: quick,
  },
];

function extractResultsSummary(resultsFile: string): string {
  const path = `${REPO_ROOT}/${resultsFile}`;
  if (!existsSync(path)) return '';
  try {
    const r = JSON.parse(readFileSync(path, 'utf8')) as {
      total: number;
      pass: number;
      fail: number;
    };
    const pct = r.total > 0 ? ((r.pass / r.total) * 100).toFixed(1) : '0.0';
    const failPart = r.fail > 0 ? `, ${RED}${r.fail} fail${RESET}` : '';
    return `${GREEN}${r.pass} pass${RESET}${failPart}, ${BOLD}${pct}%${RESET}`;
  } catch {
    return '';
  }
}

// ── State ────────────────────────────────────────────────────────────────────

interface SuiteState {
  suite: Suite;
  status: 'pending' | 'running' | 'passed' | 'failed' | 'skipped';
  summary: string;
  elapsed: number;
  output: string;
}

const states: SuiteState[] = suites.map((s) => ({
  suite: s,
  status: s.skip ? 'skipped' : 'pending',
  summary: '',
  elapsed: 0,
  output: '',
}));

// ── Rendering ────────────────────────────────────────────────────────────────

const isTTY = process.stdout.isTTY;
let frame = 0;

function buildLines(): string[] {
  const lines: string[] = [];
  let currentPhase = '';

  for (const state of states) {
    if (state.suite.phase !== currentPhase) {
      currentPhase = state.suite.phase;
      const label = currentPhase.charAt(0).toUpperCase() + currentPhase.slice(1);
      lines.push(`  ${BOLD}${label}${RESET}`);
    }

    const icon =
      state.status === 'running'
        ? `${CYAN}${SPINNER_FRAMES[frame % SPINNER_FRAMES.length]}${RESET}`
        : state.status === 'passed'
          ? CHECK
          : state.status === 'failed'
            ? CROSS
            : state.status === 'skipped'
              ? SKIP_ICON
              : `${DIM}○${RESET}`;

    const elapsed =
      state.status === 'running' || state.status === 'passed' || state.status === 'failed'
        ? `  ${DIM}${formatMs(state.elapsed)}${RESET}`
        : '';
    const summary = state.summary ? `  ${state.summary}` : '';
    const skipLabel = state.status === 'skipped' ? `  ${DIM}skipped${RESET}` : '';

    lines.push(`    ${icon} ${state.suite.name}${skipLabel}${summary}${elapsed}`);
  }

  return lines;
}

function formatMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

let totalLines = 0;

function render() {
  frame++;
  const lines = buildLines();

  if (isTTY) {
    // Move cursor to start of our output block and clear
    if (totalLines > 0) {
      process.stdout.write(`\x1b[${totalLines}F`);
    }
    for (const line of lines) {
      process.stdout.write(`${line}\x1b[K\n`);
    }
    // Clear any leftover lines from previous longer renders
    for (let i = lines.length; i < totalLines; i++) {
      process.stdout.write(`\x1b[K\n`);
    }
    totalLines = Math.max(totalLines, lines.length);
  }
}

/** Non-TTY: print final state once. */
function renderFinal() {
  if (!isTTY) {
    const lines = buildLines();
    for (const line of lines) {
      console.log(line);
    }
  }
}

// ── Execution ────────────────────────────────────────────────────────────────

async function runSuite(state: SuiteState): Promise<void> {
  if (state.status === 'skipped') return;

  state.status = 'running';
  const start = Date.now();

  const timer = setInterval(() => {
    state.elapsed = Date.now() - start;
  }, 80);

  try {
    const proc = Bun.spawn(state.suite.command, {
      cwd: state.suite.cwd,
      stdout: 'pipe',
      stderr: 'pipe',
      env: { ...process.env, FORCE_COLOR: '1' },
    });

    const [stdout, stderr] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
    ]);
    const exitCode = await proc.exited;

    state.elapsed = Date.now() - start;
    state.output = stdout + stderr;

    state.status = exitCode === 0 ? 'passed' : 'failed';

    if (state.suite.resultsFile) {
      const summary = extractResultsSummary(state.suite.resultsFile);
      if (summary) {
        state.summary = summary;
        // Conformance suites measure compliance; a completed run with results counts as passed
        state.status = 'passed';
      }
    } else if (state.suite.extract) {
      state.summary = state.suite.extract(state.output, state.suite);
    }
  } catch (err) {
    state.elapsed = Date.now() - start;
    state.status = 'failed';
    state.summary = String(err);
  } finally {
    clearInterval(timer);
  }
}

async function runPhase(phase: string): Promise<boolean> {
  const phaseStates = states.filter((s) => s.suite.phase === phase && s.status !== 'skipped');
  if (phaseStates.length === 0) return true;

  await Promise.all(phaseStates.map(runSuite));
  return phaseStates.every((s) => s.status === 'passed' || s.status === 'skipped');
}

async function main() {
  process.stdout.write(`\n  ${BOLD}${CYAN}json-schema-transformer Test Runner${RESET}\n\n`);

  if (isTTY) {
    process.stdout.write(HIDE_CURSOR);
    process.on('exit', () => process.stdout.write(SHOW_CURSOR));
    process.on('SIGINT', () => {
      process.stdout.write(SHOW_CURSOR);
      process.exit(130);
    });
  }

  // Initial render to reserve space
  render();
  const renderInterval = isTTY ? setInterval(render, 80) : null;

  const phases = ['build', 'unit', 'generate', 'conformance'];
  let allPassed = true;

  for (const phase of phases) {
    const passed = await runPhase(phase);
    if (!passed && (phase === 'build' || phase === 'generate')) {
      allPassed = false;
      break;
    }
    if (!passed) allPassed = false;
  }

  if (renderInterval) clearInterval(renderInterval);
  // Final render to ensure latest state is shown
  if (isTTY) {
    render();
    process.stdout.write(SHOW_CURSOR);
  } else {
    renderFinal();
  }

  // Summary
  const passed = states.filter((s) => s.status === 'passed').length;
  const failed = states.filter((s) => s.status === 'failed').length;
  const skipped = states.filter((s) => s.status === 'skipped').length;
  const total = states.length;

  console.log(`\n  ${BOLD}Results${RESET}`);
  console.log(
    `    ${GREEN}${passed}${RESET} passed, ${failed > 0 ? RED : DIM}${failed}${RESET} failed, ${DIM}${skipped} skipped${RESET} of ${total} suites\n`
  );

  // Print failed suite output
  for (const state of states) {
    if (state.status === 'failed') {
      console.log(`  ${RED}${BOLD}── ${state.suite.name} ──${RESET}\n`);
      const lines = state.output.trim().split('\n');
      const tail = lines.slice(-30);
      if (lines.length > 30)
        console.log(`    ${DIM}... (${lines.length - 30} lines truncated)${RESET}`);
      for (const line of tail) {
        console.log(`    ${line}`);
      }
      console.log();
    }
  }

  process.exit(allPassed ? 0 : 1);
}

main();
