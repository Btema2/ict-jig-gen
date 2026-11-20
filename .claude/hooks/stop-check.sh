#!/usr/bin/env bash
# Stop hook: refuse to stop while the crate does not compile or has clippy warnings.
# Exit 2 = block the stop and feed stderr back to Claude. Read-only: never edits files.
# Intentionally does NOT run tests (slow); `just check` is the full verification.

cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0
[[ -f Cargo.toml ]] || exit 0
command -v cargo >/dev/null 2>&1 || exit 0

if ! out=$({
    cargo check --all-targets --color never --quiet &&
        cargo clippy --all-targets --color never --quiet -- -D warnings
} 2>&1); then
    {
        echo "cargo check / clippy failed. Fix before stopping:"
        printf '%s\n' "$out" | tail -n 40
    } >&2
    exit 2
fi
exit 0
