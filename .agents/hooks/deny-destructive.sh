#!/usr/bin/env bash
# agy PreToolUse (run_command): hard-block destructive commands, mirroring the deny rules in
# .claude/settings.json. Also catches them inside chains (`cd x && rm -rf y`).
# Never escalates: anything not blocked returns "allow" (= defer to the normal policy).

cmd=$(jq -r '.toolCall.args.CommandLine // empty' 2>/dev/null) || cmd=""

block() {
    jq -nc --arg r "$1" '{decision: "deny", reason: $r}'
    exit 0
}

sep='(^|[;&|(]|[[:space:]])'
# rm with both -r and -f in any flag order/grouping (-rf, -fr, -r -f, -Rf, --recursive --force)
if [[ "$cmd" =~ ${sep}rm[[:space:]]+(.*[[:space:]])?(-[a-zA-Z]*[rR][a-zA-Z]*[[:space:]]+(.*[[:space:]])?-[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*[[:space:]]+(.*[[:space:]])?-[a-zA-Z]*[rR]|-[a-zA-Z]*([rR][a-zA-Z]*f|f[a-zA-Z]*[rR])|--recursive.*--force|--force.*--recursive) ]]; then
    block "Blocked by project policy: recursive forced rm. Delete specific files, or ask the user."
fi
if [[ "$cmd" =~ ${sep}git[[:space:]]+(.*[[:space:]])?push([[:space:]]|$) ]] &&
    [[ "$cmd" =~ [[:space:]](--force|-f)([[:space:]=]|$) || "$cmd" =~ --force-with-lease ]]; then
    block "Blocked by project policy: force push."
fi
if [[ "$cmd" =~ ${sep}git[[:space:]]+(.*[[:space:]])?reset[[:space:]]+(.*[[:space:]])?--hard ]]; then
    block "Blocked by project policy: git reset --hard."
fi

echo '{"decision":"allow"}'
