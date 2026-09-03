#!/usr/bin/env bash
# Run the pydantic conformance harness inside its venv.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ ! -x "$DIR/.venv/bin/python" ]]; then
    echo "error: venv missing — create it with:" >&2
    echo "  python3 -m venv $DIR/.venv && $DIR/.venv/bin/pip install pydantic" >&2
    exit 1
fi

source "$DIR/.venv/bin/activate"
exec python "$DIR/run.py" "$@"
