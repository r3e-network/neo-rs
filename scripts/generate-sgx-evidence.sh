#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-./tee_data}"
SGX_SDK_PATH="${SGX_SDK:-/tmp/intel-sgxsdk/sgxsdk}"
HELPER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../tools/sgx-evidence-helper" && pwd)"

if [[ ! -e /dev/sgx_enclave ]]; then
  echo "error: /dev/sgx_enclave not found" >&2
  exit 1
fi

if [[ ! -e /dev/sgx_provision && ! -e /dev/sgx/provision ]]; then
  echo "error: SGX provisioning device not found (/dev/sgx_provision)" >&2
  exit 1
fi

if [[ ! -d "$SGX_SDK_PATH" ]]; then
  echo "error: SGX SDK path not found: $SGX_SDK_PATH" >&2
  echo "hint: install SGX SDK and set SGX_SDK=/path/to/sgxsdk" >&2
  exit 1
fi

echo "building SGX evidence helper with SGX_SDK=$SGX_SDK_PATH"
make -C "$HELPER_DIR" SGX_SDK="$SGX_SDK_PATH" -j"$(nproc)"

echo "generating SGX quote and sealing key into $OUT_DIR"
"$HELPER_DIR/sgx_evidence_helper" "$OUT_DIR"

echo
echo "generated:"
echo "  $OUT_DIR/sgx.quote"
echo "  $OUT_DIR/sgx.sealing_key"
