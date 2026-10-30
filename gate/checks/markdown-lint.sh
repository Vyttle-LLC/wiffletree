#!/usr/bin/env bash
# Markdown lint: markdownlint-cli2 with the gate's config, so every brain lints
# alike. Each finding keeps its file, line, rule and description; markdownlint's
# `[Context: …]`, which quotes the file's text, is dropped.
set -euo pipefail

config="$(cd "$(dirname "$0")/../config" && pwd)/markdownlint-cli2.jsonc"
status=0
output="$(markdownlint-cli2 --config "$config" 2>&1)" || status=$?

finding='^([^:]+):([0-9]+)(:[0-9]+)? (error|warning) ([A-Z]+[0-9]+)/'
while IFS= read -r line; do
  line="${line%% \[Context: *}"
  printf '%s\n' "$line"
  if [[ "${GITHUB_ACTIONS:-}" == true && "$line" =~ $finding ]]; then
    echo "::error file=${BASH_REMATCH[1]},line=${BASH_REMATCH[2]}::markdownlint ${BASH_REMATCH[5]}"
  fi
done <<< "$output"

exit "$status"
