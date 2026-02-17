#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  echo "usage: $0 <url> <output_file> <expected_md5>" >&2
  exit 2
fi

URL="$1"
OUT="$2"
EXPECTED_MD5="$3"

mkdir -p "$(dirname "$OUT")"

server_length() {
  curl -fsSLI "$URL" | awk 'BEGIN{IGNORECASE=1} /^content-length:/ {gsub("\r","",$2); print $2; exit}'
}

local_length() {
  if [[ -f "$OUT" ]]; then
    stat -f "%z" "$OUT"
  else
    echo 0
  fi
}

echo "[$(date '+%F %T')] downloader-start url=$URL out=$OUT expected_md5=$EXPECTED_MD5"

while true; do
  echo "[$(date '+%F %T')] resume-at-bytes=$(local_length)"
  if ! curl -L --fail --retry 50 --retry-delay 5 -C - -o "$OUT" "$URL"; then
    echo "[$(date '+%F %T')] curl-failed; retrying in 5s"
    sleep 5
    continue
  fi

  actual_md5="$(md5 -q "$OUT")"
  if [[ "$actual_md5" == "$EXPECTED_MD5" ]]; then
    echo "[$(date '+%F %T')] download-complete md5-match=$actual_md5 size=$(local_length)"
    exit 0
  fi

  srv_len="$(server_length || echo 0)"
  loc_len="$(local_length)"
  echo "[$(date '+%F %T')] md5-mismatch expected=$EXPECTED_MD5 actual=$actual_md5 local_size=$loc_len server_size=$srv_len"

  if [[ "$srv_len" =~ ^[0-9]+$ ]] && (( loc_len >= srv_len )); then
    bad_out="${OUT}.bad.$(date '+%Y%m%d-%H%M%S')"
    mv "$OUT" "$bad_out"
    echo "[$(date '+%F %T')] moved-corrupt-file-to=$bad_out and restarting from scratch"
  else
    echo "[$(date '+%F %T')] continuing resume download to fix mismatch"
  fi

  sleep 2
done
