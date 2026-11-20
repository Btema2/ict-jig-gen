#!/usr/bin/env bash
# agy PostToolUse (file write tools): rustfmt the edited file if it is a .rs file.
# Non-blocking: any problem is ignored. Output must be an empty JSON object.

file=$(jq -r '.toolCall.args.TargetFile // empty' 2>/dev/null) || file=""

if [[ "$file" == *.rs && -f "$file" ]] && command -v rustfmt >/dev/null 2>&1; then
    rustfmt "$file" >/dev/null 2>&1 || true
fi
echo '{}'
