#!/usr/bin/env bash
# Stop hook: refuse to stop while the crate does not compile or has clippy warnings.
# Exit 2 = block the stop and feed stderr back to Claude. Read-only: never edits files.
# Reads the hook JSON on stdin; stop_hook_active=true means a previous block already happened.
# Intentionally does NOT run tests (slow); `just check` is the full verification.

# Loop guard: Claude Code sets stop_hook_active=true when it is already continuing because a
# Stop hook blocked. If the failure cannot be fixed (broken toolchain, offline registry, ...),
# blocking again would loop forever, so let the stop through the second time round.
payload=$(cat 2>/dev/null || true)
if command -v jq >/dev/null 2>&1; then
    [[ $(jq -r '.stop_hook_active // false' <<<"$payload" 2>/dev/null) == "true" ]] && exit 0
elif grep -Eq '"stop_hook_active"[[:space:]]*:[[:space:]]*true' <<<"$payload"; then
    exit 0
fi

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
