# Part 15 Cryptographic Integrity, Key Management, and Protected Payload Completion

Part 15 is implemented at Aequora's database-neutral reusable boundary. The framework owns
canonical cryptographic composition and verification; deployments own credentials, authorization,
provider custody, platform secure storage, and recovery operations.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Layer separation | Transport TLS remains outside aequora-crypto; typed digest, signature, artifact trust, application-level encryption, and opaque E2E contracts are separate APIs. |
| Canonical hashing | Length-framed BLAKE3 domain separation and non-interchangeable digest newtypes cover snapshots, operations, audit checkpoints, blobs, artifacts, registries, ciphertext, and plaintext. |
| Artifact authenticity | SignedArtifactManifest, SignedCheckpoint, TrustContext, and fail-closed verification bind artifact type, format, policy, tenant, content digest, purpose, key, and signing time. |
| Signing providers | KeyProvider separates private-key custody from public metadata. Ed25519 secrets are non-serializable, redacted, and zeroized. The in-memory provider is explicitly test/development-only. |
| Purpose separation | Signing and encryption purposes are disjoint; provider calls reject purpose substitution and inactive keys. |
| Registry and rotation | Root-signed complete KeyRegistryManifest generations reject rollback, duplicates, invalid lifecycle metadata, and multiple active keys per tenant/purpose. Immutable key IDs and verification-only/history rules preserve old artifacts. |
| Revocation | New use requires Active; verification evaluates signature time against validity, revocation, and known compromise metadata. Destroyed keys never verify or decrypt. |
| Device operations | Canonical signed-operation input includes retry-stable semantic fields and excludes volatile trace metadata. Device registry, device/session binding hooks, key validity, signature checks, and operation digest support ledger immutability. Authentication and authorization remain mandatory host steps. |
| Tenant encryption | XChaCha20-Poly1305 uses a fresh OS-generated 192-bit nonce and canonical AAD binding tenant, purpose, resource, field, and schema. Ciphertext and optional plaintext digests remain distinct. |
| Exports and archives | Argon2id derives one-time export keys from non-empty passphrases and random salts. Snapshot, export, replay, and audit archive purposes are distinct and reuse the envelope API. |
| E2E boundary | ProtectedDomainPolicy rejects client-managed opaque payloads whenever validation, merge, search, scoping, analytics, or content authorization needs plaintext. Key epochs and envelope structure are explicit; group ratchets remain application protocols. |
| Governance | KeyReferenceIndex, KeyDestructionEvidence, KeyDestructionRequest, and CryptoKeyStore prevent a false erasure claim while usable backup/recovery copies or plaintext caches remain. Required ciphertext blocks ordinary cleanup; explicit authorized erasure is separately classified. |
| Audit/replay | Secret-free lifecycle events expose key ID, purpose, status event, time, and authorization digest for canonical audit declarations. Artifact and operation envelopes serialize only public metadata/signatures/ciphertext, never private keys or DEKs. |
| Policy/configuration | Versioned digest/signature/encryption allowlists and required/preferred/disabled policy are part of AequoraConfig. Required capability checks reject silent snapshot/device-signature downgrade. |
| Provider composition | AequoraCrypto::builder() validates policy and required signer availability while KMS/HSM/platform implementations remain optional adapters. |
| Observability | Payload-free counters cover artifact verification, signature/decrypt failures, rotation, revocation, policy rejection, and highest accepted registry generation without key-ID labels. |
| Operations | aequora-dev crypto policy shows the safe default; crypto registry-verify verifies a root-and-registry RON bundle without exposing secrets. |
| Invariants/tests | AEQ-INV-CRYPTO001 through CRYPTO009, deterministic vectors, property tests, tamper/cross-tenant/rotation/revocation/key-loss tests, a testkit contract, and a fuzz decoder are in CI. |

## Exact constructions

Canonical values use postcard. Every digest starts with AEQUORA:DOMAIN-DIGEST:v1, followed by
little-endian lengths and the explicit versioned domain string and bytes. Artifact signatures cover
the canonical unsigned manifest under AEQUORA:SIGNED-ARTIFACT-MANIFEST:v1. Device signatures cover
the semantic operation under AEQUORA:SIGNED-OP:v1; transport trace ID is deliberately excluded.
Encryption authenticates postcard (aad_version, AssociatedData) and stores nonce, algorithm,
immutable key ID, ciphertext digest, and optional plaintext digest.

## Host and provider responsibilities

Applications still authenticate sessions, authorize every operation/artifact request, classify E2E
domains, register domain data classes, emit canonical business-audit events, choose retention,
approve erasure, and protect local client keys. Production providers must implement workload
identity, KMS/HSM/keystore access policy, rate limits, outage behavior, rotation scheduling,
backups, recovery, separation of duties, and destructive-operation authorization.

Production acceptance must verify provider-specific nonce/key generation, native audit delivery,
KMS outage behavior, backup/restore with keys, revocation propagation, provider-side destruction of
all wraps/copies, external checkpoint anchoring, and platform secure storage. Those checks cannot be
truthfully claimed by a database-neutral library without deployment credentials and infrastructure.
