#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required" >&2
  exit 2
fi

RS_GLOBS=(
  --glob '*.rs'
  --glob '!**/target/**'
  --glob '!**/.git/**'
  --glob '!**/.omx/**'
)

PROD_GLOBS=(
  "${RS_GLOBS[@]}"
  --glob '!**/tests/**'
  --glob '!**/benches/**'
  --glob '!fuzz/**'
)

count_rs_files() {
  find . \
    -path './.git' -prune -o \
    -path './target' -prune -o \
    -path './.omx' -prune -o \
    -path './*/target' -prune -o \
    -name '*.rs' -print | wc -l | tr -d ' '
}

count_workspace_members() {
  cargo metadata --no-deps --format-version 1 2>/dev/null \
    | rg -o '"workspace_members":\[[^]]*' \
    | rg -o 'path\+file://' \
    | wc -l \
    | tr -d ' '
}

print_count_by_crate() {
  local title="$1"
  local pattern="$2"
  shift 2

  echo
  echo "## ${title}"
  local matches
  matches="$(rg -l "$pattern" "$@" 2>/dev/null | cut -d/ -f1 | sort | uniq -c | sort -nr || true)"
  if [[ -z "$matches" ]]; then
    echo
    echo "_No matches._"
    return
  fi
  echo
  echo '```text'
  echo "$matches"
  echo '```'
}

print_top_files() {
  echo
  echo "## Largest Rust Files"
  echo
  echo '```text'
  set +o pipefail
  find . \
    -path './.git' -prune -o \
    -path './target' -prune -o \
    -path './.omx' -prune -o \
    -path './*/target' -prune -o \
    -name '*.rs' -print0 \
    | xargs -0 wc -l \
    | sort -nr \
    | head -40
  set -o pipefail
  echo '```'
}

print_helper_paths() {
  echo
  echo "## Helper/Utils/Context Path Names"
  echo
  echo '```text'
  find . \
    -path './.git' -prune -o \
    -path './target' -prune -o \
    -path './.omx' -prune -o \
    -path './*/target' -prune -o \
    -name '*.rs' -print \
    | rg '/(utils?|helpers?|misc|manager|runner|context)\.rs$|/(utils?|helpers?|misc)/' || true
  echo '```'
}

print_git_hygiene() {
  echo
  echo "## Tracked Runtime Data Risk"
  echo
  echo '```text'
  git ls-files \
    | rg '(^|/)(chain\.[0-9]+\.acc|chain\.acc|[^/]+\.(sst|ldb)|MANIFEST-[0-9]+|CURRENT|LOCK|LOG(\.old)?)$|(^|/)(Data_MPT[^/]*|checkpoints?|target)(/|$)' \
    || true
  echo '```'
}

echo "# neo-rs Style Conformance Audit"
echo
echo "Generated: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
echo
echo "- Workspace members: $(count_workspace_members)"
echo "- Rust files scanned: $(count_rs_files)"
echo "- Production scans exclude tests, benches, fuzz, target, .git, and .omx."
echo "- Criteria: docs/coding-design-architecture-guidance.md plus Apollo Rust best-practices categories."

print_count_by_crate \
  "Trait Object / Dynamic Dispatch Sites" \
  '\b(Arc|Box|Rc)<\s*dyn\b|&\s*dyn\b|dyn\s+[A-Z][A-Za-z0-9_]*' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "serde_json::Value / Raw JSON Boundary Sites" \
  'serde_json::Value|\bValue\b' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "Production unwrap/expect Sites" \
  '\.(unwrap|expect)\s*\(' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "Lint Allow/Expect Sites" \
  '#\s*\[allow\(|#\s*\[expect\(' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "Production panic/todo/unimplemented Sites" \
  '\b(panic!|todo!|unimplemented!)\s*\(' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "Clone Sites" \
  '\.clone\s*\(|Arc::clone|Rc::clone|\.cloned\s*\(' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "Eager Fallback Sites" \
  '\.(ok_or|unwrap_or|map_or|or)\s*\(' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "TODO/FIXME Comment Sites" \
  '\b(TODO|FIXME)\b' \
  "${PROD_GLOBS[@]}"

print_count_by_crate \
  "Trait Definition Sites" \
  '^\s*(pub\s+)?trait\s+[A-Z][A-Za-z0-9_]*' \
  "${PROD_GLOBS[@]}"

print_top_files
print_helper_paths
print_git_hygiene
