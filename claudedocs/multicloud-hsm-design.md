# Multi-cloud HSM support for neo-rs

**Status:** Design addendum (additive to the AWS CloudHSM PKCS#11 design)
**Date:** 2026-06-13
**Scope:** Generalize HSM-backed validator key custody from AWS CloudHSM to **AWS + Azure + GCP**.
**Constraint:** Neo N3 signs with **secp256r1 (NIST P-256 / prime256v1) ECDSA**, producing a **64-byte `r || s`** signature. Any product that only offers RSA, Ed25519, or secp256k1 (P-256K) is **disqualifying**. All three target products below confirm P-256 ECDSA.

---

## 1. Unifying thesis

The original AWS design defines a generic `Pkcs11Signer` driven by the [`cryptoki`](https://crates.io/crates/cryptoki) crate (a Rust-native PKCS#11 v2.40 wrapper), with the `!Send` PKCS#11 session owned by a dedicated thread, implementing:

```rust
trait ConsensusSigner {
    fn sign(&self, data: &[u8], script_hash: &UInt160) -> Result<Vec<u8>>;
}
```

**The thesis holds.** PKCS#11 is the common waist across all three clouds; a single `Pkcs11Signer` plus per-provider config covers them, because every provider exposes the same call flow:

```
load module → C_Initialize → C_GetSlotList → C_OpenSession → C_Login(CKU_USER)
→ C_FindObjects (locate key by CKA_LABEL/CKA_ID) → C_SignInit(CKM_ECDSA) → C_Sign
```

The four things that vary per provider are exactly the four the thesis predicted:

| Variant | AWS CloudHSM | Azure Cloud HSM | GCP Cloud KMS |
|---|---|---|---|
| (a) PKCS#11 `.so` + path | Cavium/Marvell `libcloudhsm_pkcs11.so` | `libazcloudhsm_pkcs11.so` (`/opt/azurecloudhsm/lib/`) | `libkmsp11.so` (`/opt/kmsp11/`) |
| (b) login / credential model | HSM-local CU username+password | HSM-local CU username+password | ADC / Workload Identity / SA JSON (env, not `C_Login` password) |
| (c) key naming / id | `CKA_LABEL` / `CKA_ID` | `CKA_LABEL` / `CKA_ID` | `CKA_LABEL` = CryptoKey id; `CKA_ID` = full CryptoKeyVersion resource name |
| (d) native fallback needed? | No | Yes — Managed HSM / Key Vault Premium expose only REST (their PKCS#11 lib is TLS-offload-only) | Optional — operators may prefer ADC + `AsymmetricSign` over FFI |

### The ONE real wrinkle: signature format

This is the only place a generic byte-for-byte signer is **not** sufficient. Neo requires raw `r || s` (64 bytes). Providers disagree on what they return:

| Provider / path | Returns | Needs DER decode? | Needs low-s normalize? |
|---|---|---|---|
| **AWS CloudHSM** PKCS#11 (`CKM_ECDSA`) | **raw `r \|\| s`** (64 B) | **No** | Recommended (see below) |
| **Azure Cloud HSM** PKCS#11 (`CKM_ECDSA`) | **raw `r \|\| s`** (64 B) | **No** | Recommended |
| **Azure Key Vault / Managed HSM** REST (`ES256`) | **raw `r \|\| s`** (64 B, base64url; ES256 per RFC 7518/JWA) | **No** | Recommended |
| **GCP Cloud KMS** PKCS#11 (`libkmsp11`, `CKM_ECDSA`) | **DER / ASN.1** `SEQUENCE { INTEGER r, INTEGER s }` (X9.62 / RFC 3279 §2.2.3) | **YES** | Recommended |
| **GCP Cloud KMS** native (`AsymmetricSign`) | **DER / ASN.1** (same) | **YES** | Recommended |

**Precise statement:** **Only GCP needs DER decoding** — on *both* its PKCS#11 and native paths. AWS and Azure (all paths) already return raw `r || s`. So the post-processing hook is GCP-specific, but the design installs it generically.

**Low-s normalization.** PKCS#11/KMS HSMs may return a high-s signature. Neo C# parity: `Crypto.VerifySignature` historically accepts high-s for secp256r1 (the recent neo-rs fix `0a9b83f3` accepted high-s for secp256k1; P-256 verification does not *reject* high-s), so low-s is **not strictly required for validity**. However, for canonical/deterministic signatures and to avoid third-party verifier rejection, the signer **should** normalize s to the low half-order (`s' = n - s` if `s > n/2`). This is cheap and provider-independent; install it as the second stage of the post-processing hook so DER-decoded GCP output and raw AWS/Azure output both pass through it. GCP notes secp256k1 keys are auto-normalized to low-s but **P-256 is not**, so we must do it ourselves for the target curve.

> **Implementation:** use `p256::ecdsa::Signature::from_der(der).to_bytes()` to get the exact 64-byte `r || s` (validates the curve), then `Signature::normalize_s()` for the low-s form. For already-raw input, parse with `Signature::from_slice(&raw)` then normalize.

---

## 2. The provider-agnostic design

The core `Pkcs11Signer` stays generic. Provider differences live in a small `ProviderProfile`. A config enum selects which profile to build.

```rust
/// Selects which provider profile (lib path, cred adapter, key-id scheme, sig post-proc) to use.
pub enum HsmProvider {
    Aws,               // libcloudhsm_pkcs11.so, CU login, label/id, raw r||s
    AzureManagedHsm,   // native REST (no general-purpose PKCS#11 lib); ES256 raw r||s
    AzureDedicatedHsm, // PKCS#11: Azure Cloud HSM (libazcloudhsm_pkcs11.so) OR Luna; CU login, raw r||s
    GcpCloudHsm,       // libkmsp11.so, ADC/env cred, label/version-resource, DER -> r||s
    GenericPkcs11,     // any other PKCS#11 HSM (Luna, nCipher, SoftHSM); user-supplied profile
}

/// The small per-provider differences. Core signer is otherwise identical.
pub struct ProviderProfile {
    pub library_path: PathBuf,            // (a) which .so to dlopen
    pub login: LoginAdapter,             // (b) CuPassword { user, secret_source } | EnvCredentials | None
    pub key_id: KeyIdScheme,             // (c) Label(String) | LabelAndId{..} | VersionResource(String)
    pub sig_post: SigPostProcessing,     // (d) RawRS | DerToRawRS ; both then low-s normalize
    pub init_args: Option<Pkcs11InitArgs>, // e.g. GCP KMS_PKCS11_CONFIG via pInitArgs->pReserved
}
```

`Pkcs11Signer::sign()` runs the universal flow on the dedicated session thread, then applies `sig_post`:

```rust
fn finalize(raw_or_der: Vec<u8>, post: &SigPostProcessing) -> Result<Vec<u8>> {
    let sig = match post {
        SigPostProcessing::RawRS      => Signature::from_slice(&raw_or_der)?,
        SigPostProcessing::DerToRawRS => Signature::from_der(&raw_or_der)?, // GCP only
    };
    let sig = sig.normalize_s().unwrap_or(sig); // canonical low-s
    Ok(sig.to_bytes().to_vec())                 // 64-byte r||s for Neo
}
```

### `[hsm]` TOML per provider

**AWS CloudHSM (PKCS#11):**
```toml
[hsm]
provider = "aws"
pkcs11.library = "/opt/cloudhsm/lib/libcloudhsm_pkcs11.so"
pkcs11.slot = 0
key.label = "neo-validator-1"
key.id = ""                                  # optional CKA_ID
cu_user = "crypto_user_1"
cu_password_source = "env:AWS_CHSM_CU_PASSWORD"   # secret indirection, never inline
# sig format: raw r||s (no DER decode); low-s normalized
```

**Azure Cloud HSM / "Dedicated" PKCS#11 (PKCS#11 via cryptoki):**
```toml
[hsm]
provider = "azure"
azure_mode = "cloud_hsm"                      # PKCS#11 path
pkcs11.library = "/opt/azurecloudhsm/lib/libazcloudhsm_pkcs11.so"
pkcs11.config_file = "/etc/azurecloudhsm/azcloudhsm_application.cfg"  # cluster/private-link
pkcs11.slot = 0
key.label = "neo-validator-1"
key.id = ""                                   # optional CKA_ID
cu_user = "crypto_user_1"
cu_password_source = "env:AZ_CHSM_CU_PASSWORD"
# sig format: raw r||s (no DER decode); low-s normalized
```

**Azure Managed HSM / Key Vault Premium (native REST):**
```toml
[hsm]
provider = "azure"
azure_mode = "managed_hsm"                    # native REST path (no general-purpose PKCS#11)
vault_url = "https://<name>.managedhsm.azure.net"   # or https://<name>.vault.azure.net (KV Premium)
key.name = "neo-validator-1"
key.version = ""                              # empty = latest
sign_alg = "ES256"                            # ECDSA P-256 / SHA-256, returns raw r||s
auth.kind = "managed_identity"                # "managed_identity" | "client_secret" | "default"
auth.client_id = ""                           # user-assigned MI, or SP app id
auth.tenant_id = ""                           # for client_secret/cert
auth.client_secret_source = "env:AZ_CLIENT_SECRET"   # or auth.client_cert_path
# sig format: raw r||s (ES256/JWA); low-s normalized
```

**GCP Cloud KMS (PKCS#11 via libkmsp11, OR native):**
```toml
[hsm]
provider = "gcp"

[hsm.gcp]
mode = "pkcs11"                               # "pkcs11" | "native"
# --- PKCS#11 path ---
pkcs11_lib  = "/opt/kmsp11/libkmsp11.so"      # from GoogleCloudPlatform/kms-integrations releases (v1.9)
config_yaml = "/etc/neo/kmsp11.yaml"          # exported as KMS_PKCS11_CONFIG; MUST be mode 0600/0400
key_label   = "neo-validator"                 # CKA_LABEL = Cloud KMS CryptoKey id
# --- native path (alternative) ---
key_resource = "projects/P/locations/L/keyRings/R/cryptoKeys/neo-validator/cryptoKeyVersions/1"
# --- auth (usually via env: GOOGLE_APPLICATION_CREDENTIALS or metadata server) ---
credentials_file = "/etc/neo/sa.json"         # optional; else ADC / Workload Identity
# sig format: DER -> r||s (REQUIRED); low-s normalized
```

The `kmsp11.yaml` referenced above:
```yaml
tokens:
  - key_ring: "projects/P/locations/L/keyRings/R"
    label: "neo-validator"
refresh_interval_secs: 86400   # 0 = cache forever (default); ring is read & cached at C_Initialize
```

---

## 3. Native-API backends

Where PKCS#11 is awkward, an optional native backend sits behind the **same** `ConsensusSigner` trait, so the consensus layer is unchanged. Both are async (no `!Send` session, no dedicated thread); bridge to the dedicated-thread model with `block_on` / a current-thread runtime if the call site is sync.

### `AzureKeyVaultSigner` (feature `hsm-azure`)
- **When:** product is **Managed HSM** or **Key Vault Premium** — their first-class (and only general-purpose) signing interface is REST. Their PKCS#11 lib (`mhsm-pkcs11`) is restricted by Microsoft to **SSL/TLS offload with F5/Nginx only**, so it is *not* a usable general signer.
- **Call:** `POST https://{vault}.vault.azure.net/keys/{name}/{version}/sign?api-version=7.4` with `{"alg":"ES256","value":"<base64url SHA-256 digest>"}`. neo-rs computes the 32-byte SHA-256 digest locally (the service signs a *hash*, not raw data). Response `value` is base64url **raw `r || s`** — directly Neo-compatible.
- **Crates:** `azure_security_keyvault_keys = "1.0"` (sign op), `azure_identity = "1.0"` (`ManagedIdentityCredential` / `ClientSecretCredential` / `DefaultAzureCredential` → `Arc<dyn TokenCredential>`), `azure_core = "1.0"` (pipeline, base64url).

### `GcpKmsSigner` (feature `hsm-gcp`)
- **When:** operators prefer pure-Rust ADC / Workload Identity over installing `libkmsp11.so` (no FFI, no `C_Initialize` global state, no key-ring cache staleness, explicit per-call resource name).
- **Call:** `KeyManagementService/AsymmetricSign { name: <CryptoKeyVersion>, digest: { sha256: <32-byte digest> } }`. Response `signature` is **DER** → run the same DER→`r||s` + low-s normalize.
- **Crates:** `google-cloud-kms = "0.6"` (yoshidan; async/tonic, pragmatic) **or** `google-cloud-kms-v1 = "1.11"` (official googleapis/google-cloud-rust; long-term) — pick one, not both; plus `p256 = "0.13"` for DER→raw, and `tokio = "1"`.

---

## 4. Crate plan & feature flags

New crate **`neo-hsm`**, depending on `neo-consensus` for the `ConsensusSigner` trait.

```toml
# neo-hsm/Cargo.toml
[features]
default = ["pkcs11"]
pkcs11  = ["dep:cryptoki", "dep:p256"]          # generic signer: AWS + Azure Cloud HSM + GCP libkmsp11 + any HSM
azure   = ["dep:azure_security_keyvault_keys", "dep:azure_identity", "dep:azure_core", "dep:p256"]  # native KV/Managed-HSM REST
gcp     = ["dep:google-cloud-kms", "dep:p256", "dep:tokio"]   # native Cloud KMS AsymmetricSign

[dependencies]
cryptoki = { version = "0.12", optional = true }     # PKCS#11 v2.40 wrapper, all clouds
p256     = { version = "0.13", optional = true }     # DER<->raw r||s + low-s normalize (RustCrypto)
azure_security_keyvault_keys = { version = "1.0", optional = true }  # GA'd 2026-05
azure_identity = { version = "1.0", optional = true }               # GA'd 2026-05-12
azure_core     = { version = "1.0", optional = true }
google-cloud-kms = { version = "0.6", optional = true }             # yoshidan async client
tokio = { version = "1", optional = true }
```

- `pkcs11` (default) gives the generic `Pkcs11Signer` and covers **all three clouds** via cryptoki (AWS CloudHSM, Azure Cloud HSM, GCP libkmsp11) plus any generic HSM (Luna/nCipher/SoftHSM).
- `azure` adds `AzureKeyVaultSigner` (native REST) for Managed HSM / Key Vault Premium.
- `gcp` adds `GcpKmsSigner` (native `AsymmetricSign`) for the ADC-preferring path.
- `p256` is shared: required by the GCP PKCS#11 path (DER decode) and by both native backends; pulled in transitively wherever needed.

---

## 5. Capability matrix

| Provider | Product | P-256 ECDSA? | PKCS#11 lib | Native Rust SDK | Sig format | Auth |
|---|---|---|---|---|---|---|
| **AWS** | CloudHSM | ✅ `CKM_ECDSA` on P-256 | `libcloudhsm_pkcs11.so` (Marvell/Cavium) | — (PKCS#11 only) | **raw `r\|\|s`** | HSM-local Crypto User (CU) password via `C_Login` |
| **Azure** | Cloud HSM (GA) | ✅ `CKM_ECDSA` on P-256 | `libazcloudhsm_pkcs11.so` (full v2.40, general-purpose) | — (use PKCS#11) | **raw `r\|\|s`** | HSM-local CU password via `C_Login` |
| **Azure** | Managed HSM | ✅ `EC-HSM` P-256 + `ES256` | `mhsm-pkcs11` — **TLS-offload only, not usable** | `azure_security_keyvault_keys 1.0` (REST) | **raw `r\|\|s`** (ES256/JWA) | Entra ID OAuth2 bearer (`azure_identity`): Managed Identity / SP secret / cert |
| **Azure** | Key Vault Premium | ✅ `EC-HSM` P-256 + `ES256` | (same TLS-offload lib) | `azure_security_keyvault_keys 1.0` (REST) | **raw `r\|\|s`** | Entra ID OAuth2 bearer; RBAC "Key Vault Crypto User" |
| **Azure** | Dedicated HSM (Luna 7) | ✅ P-256 (Luna) — **EOL track, avoid** | Chrystoki/Luna client lib | — | **raw `r\|\|s`** | Luna partition login |
| **GCP** | Cloud KMS (HSM protection) | ✅ `EC_SIGN_P256_SHA256` | `libkmsp11.so` (kms-integrations v1.9) | `google-cloud-kms 0.6` / `google-cloud-kms-v1 1.11` (`AsymmetricSign`) | **DER → must convert** | ADC / Workload Identity / SA JSON (env / metadata server) |

Disqualified-for-Neo algorithms seen but NOT used: GCP `EC_SIGN_SECP256K1_SHA256` (secp256k1, wrong curve), `EC_SIGN_ED25519`, `EC_SIGN_P384_SHA384`; any RSA-only mode. All chosen products satisfy Neo's P-256 requirement.

---

## 6. Implementation note

This work is **purely additive** to the AWS design — no rework of the consensus seam.

1. **Build the generic `Pkcs11Signer` first** (the AWS deliverable): cryptoki + dedicated `!Send` session thread + `ConsensusSigner` impl + the `finalize()` post-processing hook (`RawRS` + low-s).
2. **Once it exists, Azure Cloud HSM and GCP-via-`libkmsp11` already work** by config alone — only the `.so` path, login adapter, key-id scheme, and (for GCP) flipping `sig_post` to `DerToRawRS` change. No new signing code. This is the direct payoff of the unifying thesis.
3. **Add the `DerToRawRS` post-processing** (one match arm + `p256::ecdsa::Signature::from_der`) — the single GCP-specific wrinkle.
4. **Native backends are opt-in later**, behind `hsm-azure` / `hsm-gcp`, only needed for Azure Managed HSM / Key Vault Premium (no usable PKCS#11) or operators who prefer GCP ADC over FFI. They share the same `ConsensusSigner` trait, so they drop in without touching consensus.

Ordering: generic PKCS#11 (covers AWS + Azure Cloud HSM + GCP libkmsp11) → GCP DER hook → optional native Azure REST → optional native GCP KMS.
