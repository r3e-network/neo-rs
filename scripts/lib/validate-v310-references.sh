#!/usr/bin/env bash

# Exit status used when parity cannot be evaluated because a required external
# implementation is unavailable. A mismatch continues to use status 1.
readonly V310_REFERENCE_UNREACHABLE_EXIT=75

require_v310_reference_pair() {
  local network="$1"
  local csharp_rpc="$2"
  local neogo_rpc="$3"

  if [[ -n "$csharp_rpc" && -n "$neogo_rpc" ]]; then
    return 0
  fi

  local missing=()
  [[ -z "$csharp_rpc" ]] && missing+=("Neo C# v3.10.1")
  [[ -z "$neogo_rpc" ]] && missing+=("NeoGo")
  local missing_text
  missing_text="$(IFS=', '; echo "${missing[*]}")"
  echo "[$network] REFERENCE-UNREACHABLE: required reference implementation(s) unavailable: $missing_text. Consistency was not evaluated; this is not a parity match or mismatch." >&2
  return "$V310_REFERENCE_UNREACHABLE_EXIT"
}
