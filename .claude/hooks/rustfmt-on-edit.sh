#!/usr/bin/env bash
# PostToolUse (Edit|Write): rustfmt the edited file if it is a .rs file.
# Fast and non-blocking: any problem (missing jq/rustfmt, unparsable mid-edit file) exits 0.

file=$(jq -r '.tool_input.file_path // empty' 2>/dev/null) || exit 0

[[ "$file" == *.rs && -f "$file" ]] || exit 0
command -v rustfmt >/dev/null 2>&1 || exit 0

# rustfmt finds rustfmt.toml by walking up from the file, so edition/width come from the repo.
rustfmt "$file" >/dev/null 2>&1 || true
exit 0
