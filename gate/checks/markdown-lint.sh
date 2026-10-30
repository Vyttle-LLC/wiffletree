#!/usr/bin/env bash
# Markdown lint: markdownlint-cli2 with the gate's config, so every brain lints
# alike. Each finding is printed as its file, line, column, severity and rule
# only: markdownlint's descriptions and `[…]` details can quote the file's
# text, so they are dropped, as is any other line it prints.
set -euo pipefail

config="$(cd "$(dirname "$0")/../config" && pwd)/markdownlint-cli2.jsonc"
status=0
output="$(markdownlint-cli2 --config "$config" 2>&1)" || status=$?

finding='^([^ :]+):([0-9]+)(:[0-9]+)? (error|warning) ([A-Z]+[0-9]+(/[a-z0-9-]+)*) '
findings=0
while IFS= read -r line; do
  if [[ "$line" =~ $finding ]]; then
    findings=$((findings + 1))
    printf '%s:%s%s %s %s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}" "${BASH_REMATCH[4]}" "${BASH_REMATCH[5]}"
    if [[ "${GITHUB_ACTIONS:-}" == true ]]; then
      echo "::error file=${BASH_REMATCH[1]},line=${BASH_REMATCH[2]}::markdownlint ${BASH_REMATCH[5]}"
    fi
  elif [[ "$line" =~ ^(markdownlint-cli2\ v|Linting:|Summary:) ]]; then
    printf '%s\n' "$line"
  fi
done <<< "$output"

if (( status != 0 && findings == 0 )); then
  echo "ERROR: markdownlint-cli2 could not lint (exit $status); run it locally to see why"
fi
echo "markdown-lint: $findings failure(s)"
exit "$status"
