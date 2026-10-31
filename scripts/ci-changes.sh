#!/usr/bin/env bash
# Classify a pull request for CI. Reads newline-separated paths on stdin
# (git diff --name-only) and prints mode=skip|web|full for $GITHUB_OUTPUT.
#
# skip: docs, markdown, landing page. The job still reports success.
# web:  web, packages, or the e2e runner only. Stack runs Playwright against
#       a cached profile.ci binary and does not compile Rust when that cache hits.
# full: anything else, including Rust, SQL, workflows, and docs/api.
set -euo pipefail

mode=skip
while IFS= read -r f || [ -n "${f:-}" ]; do
  [ -z "$f" ] && continue
  case "$f" in
    docs/api/*)
      mode=full
      ;;
    docs/*|*.md|apps/landing/*)
      ;;
    apps/web/*|packages/*|pnpm-lock.yaml|pnpm-workspace.yaml|package.json|scripts/e2e.mjs)
      if [ "$mode" = skip ]; then
        mode=web
      fi
      ;;
    *)
      mode=full
      ;;
  esac
done

printf 'mode=%s\n' "$mode"
