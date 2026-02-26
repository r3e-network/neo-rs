# SGX Evidence Helper

Generates real SGX quote evidence for `neo-node --features tee-sgx` strict mode.

This helper:
- Creates a random 32-byte sealing key.
- Computes `report_data[0..32] = SHA256("neo-tee-sgx-sealing-key-v1" || sealing_key)`.
- Generates an SGX report inside a real enclave.
- Gets a DCAP quote from the QE.
- Writes:
  - `sgx.quote`
  - `sgx.sealing_key`

These outputs are compatible with the strict SGX evidence checks in `neo-tee`.

## Prerequisites

- Real SGX hardware and driver (`/dev/sgx_enclave`, `/dev/sgx_provision`).
- SGX SDK installed. Default path used by this helper:
  - `/tmp/intel-sgxsdk/sgxsdk`
- DCAP quote libs installed (`libsgx_dcap_ql.so`).

## Build

```bash
cd tools/sgx-evidence-helper
make
```

Optional SDK override:

```bash
make SGX_SDK=/path/to/sgxsdk
```

## Generate Evidence

```bash
cd tools/sgx-evidence-helper
./sgx_evidence_helper /tmp/neo-tee-strict-test
```

This writes:
- `/tmp/neo-tee-strict-test/sgx.quote`
- `/tmp/neo-tee-strict-test/sgx.sealing_key`

## Run neo-node strict tee-sgx

```bash
target/debug/neo-node \
  --config neo_mainnet_node.toml \
  --tee \
  --tee-data-path /tmp/neo-tee-strict-test \
  --tee-ordering-policy batched
```

## Notes

- By default this helper creates a non-debug enclave (`SGX_DEBUG=0`).
- If you need debug enclave output for troubleshooting:

```bash
make SGX_DEBUG=1
NEO_SGX_HELPER_DEBUG_ENCLAVE=1 ./sgx_evidence_helper /tmp/neo-tee-strict-test
```
