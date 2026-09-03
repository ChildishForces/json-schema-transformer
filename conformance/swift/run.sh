#!/usr/bin/env bash
# Swift conformance harness: compile ALL generated fixtures into one binary,
# run the whole suite in one process.
#
#   conformance/swift/run.sh
#
# Files that fail to compile are excluded iteratively (their groups are marked
# all-tests-failed with the compiler error as the reason).
set -euo pipefail
cd "$(dirname "$0")"

GEN_DIR=../generated
BUILD=.build
RESULTS=../results/swift.json
EXCLUSIONS=$BUILD/exclusions.json

mkdir -p "$BUILD" ../results
echo '{}' > "$EXCLUSIONS"

NCPU=$(sysctl -n hw.ncpu 2>/dev/null || nproc)

compile_start=$SECONDS
compiled=0
for attempt in 1 2 3 4 5 6; do
  bun gen.ts gen "$GEN_DIR/manifest.json" "$EXCLUSIONS" "$BUILD/main.swift" "$BUILD/sources.txt"
  echo "compile attempt $attempt ($(wc -l < "$BUILD/sources.txt" | tr -d ' ') fixture files)..."
  if swiftc -Onone -suppress-warnings -j "$NCPU" \
      -module-name ConformanceRunner \
      -o "$BUILD/runner" \
      @"$BUILD/sources.txt" Support.swift "$BUILD/main.swift" \
      2> "$BUILD/compile.log"; then
    compiled=1
    break
  fi
  added=$(bun gen.ts exclude "$BUILD/compile.log" "$EXCLUSIONS")
  echo "  compile failed; excluded $added file(s)"
  if [ "$added" = "0" ]; then
    echo "compile failed with no attributable generated file:" >&2
    tail -40 "$BUILD/compile.log" >&2
    exit 1
  fi
done
if [ "$compiled" != "1" ]; then
  echo "still failing after exclusion attempts:" >&2
  tail -40 "$BUILD/compile.log" >&2
  exit 1
fi
compile_time=$((SECONDS - compile_start))
excluded=$(bun -e "process.stdout.write(String(Object.keys(require('./$EXCLUSIONS')).length))")
echo "compiled in ${compile_time}s (excluded $excluded file(s))"

run_start=$SECONDS
"$BUILD/runner" "$GEN_DIR/manifest.json" "$RESULTS" "$EXCLUSIONS"
echo "total run time: $((SECONDS - run_start))s (compile ${compile_time}s)"
