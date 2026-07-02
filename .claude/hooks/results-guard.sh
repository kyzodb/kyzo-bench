#!/usr/bin/env bash
# PreToolUse guard for the published-results blast-radius zone (see .claude/rules/results-data.md).
# Reads the tool-call JSON from stdin. If the edit targets an EXISTING file under results/, that is
# a violation of the append-only invariant: emit additionalContext warning the agent. New files
# under results/ are fine. Silent (exit 0, no output) otherwise.
#
# Test locally:
#   echo '{"tool_input":{"file_path":"results/oltp/2026-07-01-run.json"}}' | .claude/hooks/results-guard.sh
set -euo pipefail

file=$(jq -r '.tool_input.file_path // ""')

case "$file" in
  *results/*)
    if [ -e "$file" ] || [ -e "${CLAUDE_PROJECT_DIR:-.}/${file#*kyzo-bench/}" ]; then
      jq -cn '{hookSpecificOutput:{hookEventName:"PreToolUse",additionalContext:"results/ is APPEND-ONLY published data: a committed result file is never edited or deleted (see .claude/rules/results-data.md). Corrections are NEW files with a supersedes: header naming the flawed one. If this edit targets an existing results file, stop."}}'
    fi
    ;;
esac
exit 0
