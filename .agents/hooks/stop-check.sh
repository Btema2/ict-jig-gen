#!/usr/bin/env bash
# agy Stop hook: do not let the agent finish while cargo check / clippy fail.
# Mirrors .claude/hooks/stop-check.sh. Read-only; no tests (use `just check` for those).
# Output {"decision":"continue","reason":...} re-enters the loop; anything else allows the stop.

payload=$(cat)
allow() { echo '{"decision":"stop"}'; exit 0; }

# Only gate a normal, fully finished stop. executionNum cap = loop guard (agy has no
# stop_hook_active flag); after 5 attempts let the agent stop and report instead of spinning.
[[ $(jq -r '.terminationReason // ""' <<<"$payload" 2>/dev/null) == "model_stop" ]] || allow
[[ $(jq -r '.fullyIdle // false' <<<"$payload" 2>/dev/null) == "true" ]] || allow
(($(jq -r '.executionNum // 1' <<<"$payload" 2>/dev/null) <= 5)) || allow

dir=$(jq -r '.workspacePaths[0] // empty' <<<"$payload" 2>/dev/null)
cd "${dir:-.}" 2>/dev/null || allow
[[ -f Cargo.toml ]] && command -v cargo >/dev/null 2>&1 || allow

if out=$({
    cargo check --all-targets --color never --quiet &&
        cargo clippy --all-targets --color never --quiet -- -D warnings
} 2>&1); then
    allow
fi

jq -nc --arg o "$(printf '%s\n' "$out" | tail -n 40)" \
    '{decision: "continue", reason: ("cargo check / clippy failed. Fix before finishing:\n" + $o)}'
