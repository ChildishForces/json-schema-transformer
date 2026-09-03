#!/usr/bin/env bash
# Kotlin conformance harness: generate Runner.kt from the manifest, compile
# ALL generated fixtures + runner in ONE kotlinc invocation, run the whole
# suite in one JVM, write conformance/results/kotlin.json.
#
# Usage: conformance/kotlin/run.sh
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
GEN_DIR="$ROOT/conformance/generated/kotlin"
MANIFEST="$ROOT/conformance/generated/manifest.json"
BUILD="$HERE/build"
LIBS="$HERE/libs"
RESULTS_DIR="$ROOT/conformance/results"
RESULTS="$RESULTS_DIR/kotlin.json"

KOTLINC="${KOTLINC:-kotlinc}"
if [ -z "${KOTLIN_HOME:-}" ]; then
  if [ -d /opt/homebrew/opt/kotlin/libexec/lib ]; then
    KOTLIN_HOME=/opt/homebrew/opt/kotlin/libexec
  elif [ -d "$HERE/toolchain/kotlinc/lib" ]; then
    KOTLIN_HOME="$HERE/toolchain/kotlinc"
    KOTLINC="$KOTLIN_HOME/bin/kotlinc"
  else
    KOTLIN_HOME="$(dirname "$(dirname "$(readlink -f "$(command -v "$KOTLINC")")")")"
  fi
fi
SER_PLUGIN="$KOTLIN_HOME/lib/kotlinx-serialization-compiler-plugin.jar"
STDLIB="$KOTLIN_HOME/lib/kotlin-stdlib.jar"

SER_JARS="$(ls "$LIBS"/kotlinx-serialization-core-jvm-*.jar):$(ls "$LIBS"/kotlinx-serialization-json-jvm-*.jar)"

[ -f "$SER_PLUGIN" ] || { echo "serialization compiler plugin not found: $SER_PLUGIN" >&2; exit 1; }

mkdir -p "$BUILD/classes" "$RESULTS_DIR"
rm -rf "$BUILD/classes"; mkdir -p "$BUILD/classes"

EXCLUSIONS="$BUILD/exclusions.json"
echo '{"files":{}}' > "$EXCLUSIONS"

MAX_ITERS=6
compile_ok=0
compile_start=$(date +%s)

for iter in $(seq 1 "$MAX_ITERS"); do
  # 1. (Re)generate Runner.kt honouring current exclusions
  bun "$HERE/gen-runner.js" "$MANIFEST" "$EXCLUSIONS" "$BUILD/Runner.kt"

  # 2. Collect sources minus excluded files
  SRC_LIST="$BUILD/sources.txt"
  : > "$SRC_LIST"
  excluded_names=$(bun -e '
    const e = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    console.log(Object.keys(e.files || {}).join("\n"));
  ' "$EXCLUSIONS")
  for f in "$GEN_DIR"/*.kt; do
    base="$(basename "$f")"
    if ! grep -qxF "$base" <<< "$excluded_names"; then
      echo "$f" >> "$SRC_LIST"
    fi
  done
  echo "$BUILD/Runner.kt" >> "$SRC_LIST"

  echo "[iter $iter] compiling $(wc -l < "$SRC_LIST" | tr -d ' ') files..."
  if "$KOTLINC" -nowarn \
      -Xplugin="$SER_PLUGIN" \
      -Xbackend-threads=0 \
      -cp "$SER_JARS" \
      -d "$BUILD/classes" \
      @"$SRC_LIST" > "$BUILD/kotlinc.log" 2>&1; then
    compile_ok=1
    break
  fi

  # 3. Parse errors, exclude offending generated files
  echo "[iter $iter] compile failed; error summary:"
  grep -E "error:" "$BUILD/kotlinc.log" | sed 's/^.*error:/error:/' | sort | uniq -c | sort -rn | head -10

  # kotlinc may print paths relative to its cwd, so match on the
  # generated/kotlin suffix and reduce to basenames.
  bad=$(grep -Eo "[A-Za-z0-9_/.-]*generated/kotlin/[A-Za-z0-9_]+\.kt:[0-9]+:[0-9]+: error" "$BUILD/kotlinc.log" \
        | sed -E 's|.*/([A-Za-z0-9_]+\.kt).*|\1|' | sort -u)
  if [ -z "$bad" ]; then
    echo "compile errors are not attributable to generated files (Runner.kt or toolchain problem):" >&2
    head -40 "$BUILD/kotlinc.log" >&2
    exit 1
  fi
  for base in $bad; do
    reason=$(grep -E "(^|/)$base:[0-9]+:[0-9]+: error:" "$BUILD/kotlinc.log" | head -1 | sed 's/^.*error: //' | cut -c1-200)
    bun -e '
      const fs = require("fs");
      const p = process.argv[1];
      const e = JSON.parse(fs.readFileSync(p, "utf8"));
      e.files[process.argv[2]] = "compile error: " + process.argv[3];
      fs.writeFileSync(p, JSON.stringify(e, null, 2));
    ' "$EXCLUSIONS" "$base" "$reason"
    echo "  excluding $base ($reason)"
  done
done

compile_end=$(date +%s)
if [ "$compile_ok" -ne 1 ]; then
  echo "compilation still failing after $MAX_ITERS iterations" >&2
  exit 1
fi

excl_count=$(bun -e '
  const e = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  console.log(Object.keys(e.files || {}).length);
' "$EXCLUSIONS")
echo "compile OK in $((compile_end - compile_start))s (excluded files: $excl_count)"

# 4. Run the whole suite in one JVM
run_start=$(date +%s)
java -cp "$BUILD/classes:$STDLIB:$SER_JARS" spec.events.RunnerKt "$MANIFEST" "$RESULTS"
run_status=$?
run_end=$(date +%s)
echo "compile time: $((compile_end - compile_start))s, run time: $((run_end - run_start))s"
echo "results written to $RESULTS"
exit "$run_status"
