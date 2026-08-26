# Aequora Sync — Part 15

# Cryptographic Integrity, Key Management, Signed Artifacts, and Optional End-to-End Protected Payload Architecture

## 1. Purpose

Aequora already has strong logical correctness mechanisms:

```text
OperationId idempotency
authoritative journal
canonical hashing
anti-entropy
audit chains
snapshot manifests
replay bundles
governance manifests
```

Those mechanisms answer:

```text
what state should exist?
which operation caused it?
how do replicas converge?
```

Part 15 adds cryptographic assurances for questions such as:

```text
Was this artifact modified?
Did this manifest really come from the authority?
Did this device really sign this operation?
Can old signing keys be rotated without invalidating all history?
How do we protect sensitive exported/replay/snapshot data at rest?
Can some payloads be end-to-end encrypted while keeping Aequora usable?
```

The central rule is:

> **Use cryptography to authenticate identities, protect confidentiality, and make durable artifacts tamper-evident—but never use encryption in ways that silently remove the server's ability to enforce domain invariants it is responsible for.**

---

# 2. Goals

The cryptographic architecture should provide:

```text
artifact integrity
artifact authenticity
key rotation
device key lifecycle
server signing identity
tenant-scoped encryption
snapshot/export/replay protection
audit-chain anchoring
secure key derivation
revocation
optional end-to-end protected payloads
cryptographic erasure support
```

---

# 3. Non-Goals

Aequora should not become:

```text
a custom cryptographic library
a blockchain
a PKI product
a mandatory E2E messaging system
a hardware-security-module abstraction for every deployment
```

Use well-reviewed Rust cryptography crates and platform/managed key services where appropriate.

---

# 4. Cryptographic Layers

Separate five concerns:

```text
1. Transport Security
2. Artifact Integrity
3. Artifact Authenticity
4. Data-at-Rest Confidentiality
5. Optional End-to-End Payload Protection
```

Do not collapse these into one "encryption" feature.

---

# 5. Transport Security

Normal client/server communication should use:

```text
TLS
```

over HTTPS.

TLS protects:

```text
confidentiality in transit
server authentication
transport integrity
```

Aequora protocol semantics still provide their own idempotency and integrity checks.

---

# 6. TLS Is Not Enough for Stored Artifacts

TLS does not prove that a snapshot file stored for six months has not been altered.

For durable artifacts use:

```text
cryptographic digest
optional digital signature
```

---

# 7. Hash Algorithm

Recommended general-purpose digest:

```text
BLAKE3
```

Use for:

```text
canonical entity digests
snapshot chunks
snapshot manifests
replay bundles
export bundles
governance manifests
operation semantic hashes
audit checkpoints
```

---

# 8. Hash Domain Separation

Every digest format must use a domain tag.

Example:

```text
AEQUORA:SNAPSHOT-CHUNK:v1
AEQUORA:AUDIT-EVENT:v1
AEQUORA:OPERATION:v1
AEQUORA:EXPORT:v1
```

Never hash ambiguous raw concatenations.

---

# 9. Digest Type Safety

Avoid one generic byte array everywhere.

Define newtypes:

```rust
pub struct SnapshotDigest([u8; 32]);
pub struct OperationDigest([u8; 32]);
pub struct AuditDigest([u8; 32]);
pub struct BlobDigest([u8; 32]);
```

This prevents accidental cross-domain comparisons.

---

# 10. Canonical Encoding

A digest is meaningful only if both sides agree on canonical bytes.

Therefore every hashed structure needs:

```text
stable field ordering
explicit version
stable numeric IDs
canonical numbers/timestamps
domain tag
```

---

# 11. Signature Algorithm

For signing server artifacts and device attestable operations, a suitable default is:

```text
Ed25519
```

because it has:

```text
small keys
small signatures
good Rust ecosystem support
deterministic signing
simple verification
```

Use vetted crates.

---

# 12. SigningKeyId

Define:

```rust
pub struct SigningKeyId(Uuid);
```

Every signature includes the ID of the key used.

---

# 13. SignatureEnvelope

Conceptually:

```rust
pub struct SignatureEnvelope {
    pub algorithm: SignatureAlgorithm,
    pub key_id: SigningKeyId,
    pub signed_at: Timestamp,
    pub signature: Vec<u8>,
}
```

---

# 14. Key Purpose Separation

Never use one key for everything.

Separate at least:

```text
ServerArtifactSigning
DeviceOperationSigning
TenantDataEncryption
SnapshotEncryption
ExportEncryption
AuditCheckpointSigning
```

---

# 15. KeyPurpose

```rust
pub enum KeyPurpose {
    ServerArtifactSigning,
    DeviceOperationSigning,
    TenantDataEncryption,
    AuditCheckpointSigning,
    ExportEncryption,
    SnapshotEncryption,
}
```

---

# 16. Server Artifact Signing

Server can sign:

```text
snapshot manifests
export manifests
audit checkpoints
migration manifests
governance purge verification manifests
```

This proves the artifact was issued by an authorized Aequora authority key.

---

# 17. Do Not Sign Every Journal Event by Default

Per-event signatures add:

```text
CPU cost
storage
key-management complexity
```

without always providing proportional benefit.

Recommended:

```text
hash-chained journal/audit
+
periodic signed checkpoints
```

for high-assurance deployments.

---

# 18. Signed Checkpoints

Example:

```text
journal/audit root through sequence N
↓
server signs checkpoint
```

Then later verification can prove:

```text
history up to N matches the signed root
```

---

# 19. Checkpoint Type

```rust
pub struct SignedCheckpoint {
    pub scope: CheckpointScope,
    pub sequence: u64,
    pub root_hash: Digest,
    pub signature: SignatureEnvelope,
}
```

---

# 20. Verification Key Distribution

Clients/admin tools need trusted server public keys.

Possible trust roots:

```text
bundled application trust root
server certificate-bound metadata
tenant configuration
admin-managed key registry
```

Do not trust a public key delivered inside the artifact it signs without an external trust root.

---

# 21. Server Key Registry

Authoritative metadata:

```text
key_id
purpose
public_key
status
not_before
not_after
created_at
revoked_at
```

Private key material is not stored in ordinary application tables when avoidable.

---

# 22. Private Key Storage

Deployment options:

```text
OS-protected key file
cloud KMS
HSM
TPM-backed key
secret manager
```

The Aequora architecture should abstract key use, not dictate one provider.

---

# 23. KeyProvider Trait

Conceptually:

```rust
pub trait KeyProvider {
    async fn sign(
        &self,
        purpose: KeyPurpose,
        key_id: SigningKeyId,
        digest: &[u8],
    ) -> Result<SignatureBytes, KeyError>;

    async fn public_key(
        &self,
        key_id: SigningKeyId,
    ) -> Result<PublicKeyBytes, KeyError>;
}
```

Encryption key APIs should be separate to avoid misuse.

---

# 24. Key Rotation

Keys must rotate without invalidating existing artifacts.

Rule:

```text
old artifacts retain old key_id/signature
old public key remains available for verification
new artifacts use new key
```

---

# 25. Rotation State

Key status:

```rust
pub enum KeyStatus {
    Pending,
    Active,
    Retiring,
    VerificationOnly,
    Revoked,
}
```

---

# 26. Normal Rotation

Flow:

```text
create new key
↓
publish public key
↓
mark active
↓
new signatures use new key
↓
old key becomes VerificationOnly
```

---

# 27. Revocation

If signing key compromised:

```text
mark revoked
stop new signatures
publish revocation metadata
issue new key
```

Historical artifacts signed before compromise may require:

```text
risk policy
re-signing checkpoints
external evidence review
```

Do not automatically declare all historical signatures valid forever.

---

# 28. Key Revocation Metadata

Include:

```text
revoked_at
reason
compromise_time_if_known
```

Verification can distinguish:

```text
signed before known compromise
signed after compromise
```

---

# 29. Device Key Pair

Optional stronger device identity:

```text
each device has Ed25519 key pair
```

Private key stored in:

```text
Android Keystore
iOS Keychain/Secure Enclave where possible
desktop OS secret store/TPM where available
```

---

# 30. Device Public Key Registration

Server associates:

```text
DeviceId
→
device public key
```

during authenticated registration.

---

# 31. Signed Operation Envelope

Optional:

```rust
pub struct SignedOperation {
    pub operation: OperationEnvelope,
    pub signature: DeviceSignature,
}
```

Device signs a canonical operation digest.

---

# 32. What Device Signature Proves

It can prove:

```text
operation originated from holder of registered device key
payload was not modified after signing
```

It does **not** prove:

```text
the human user intended it
device was uncompromised
operation is authorized
```

Server authorization still applies.

---

# 33. Device Signature and Idempotency

Signature covers:

```text
OperationId
tenant
actor claim/context binding
device ID
entity
base version
dependencies
payload
schema version
correlation/causation where semantic
```

Do not include volatile transport fields.

---

# 34. Signature Canonical Operation Digest

Conceptually:

```text
H(
  "AEQUORA:SIGNED-OP:v1",
  canonical_semantic_operation
)
```

---

# 35. Device Signature Verification Pipeline

```text
decode
↓
lookup registered device key
↓
verify signature
↓
authenticate session
↓
bind device/session/tenant
↓
authorize
↓
validate domain
↓
execute
```

---

# 36. Signature Is Not Authentication Replacement

A stolen operation signed months ago could be replayed.

Use:

```text
OperationId idempotency
session/device status
operation age policy
scope generation
authorization
```

---

# 37. Device Key Rotation

Reasons:

```text
OS reinstallation
key compromise
device migration
security policy
```

Process:

```text
authenticate strongly
register new public key
retire old key
```

---

# 38. Device Revocation

Revoking DeviceId should also revoke associated device signing keys.

---

# 39. Local Key Loss

If device private key lost:

```text
device must re-register
```

Pending unsent operations signed with lost key may need:

```text
re-sign with new key only if never sent and semantics allow
```

If already sent:

```text
keep original signed payload if available
```

---

# 40. Operation Signature Immutability

Part 04 rule extends:

> Once an operation may have reached the server, its semantic payload and signature mapping must remain immutable.

---

# 41. Encryption at Rest

Server-side data encryption should normally use:

```text
database/storage encryption
+
application-level envelope encryption for especially sensitive data
```

---

# 42. Envelope Encryption

Pattern:

```text
random Data Encryption Key (DEK)
↓
encrypt data
↓
wrap DEK with Key Encryption Key (KEK)
```

---

# 43. Tenant Key Hierarchy

Recommended conceptual hierarchy:

```text
Root/Provider Key
↓
Tenant KEK
↓
Purpose-specific DEKs
```

This enables:

```text
tenant isolation
rotation
cryptographic erasure
```

---

# 44. TenantKeyId

```rust
pub struct TenantKeyId(Uuid);
```

---

# 45. Encryption Metadata

Ciphertext envelope:

```rust
pub struct EncryptedPayload {
    pub algorithm: EncryptionAlgorithm,
    pub key_id: TenantKeyId,
    pub nonce: Vec<u8>,
    pub aad_version: u16,
    pub ciphertext: Vec<u8>,
}
```

---

# 46. AEAD

Use authenticated encryption.

Suitable modern choices include:

```text
AES-256-GCM
XChaCha20-Poly1305
```

depending on platform/provider.

Do not use unauthenticated encryption.

---

# 47. Additional Authenticated Data

Bind ciphertext to context.

AAD may include:

```text
tenant ID
entity ID
field ID
schema version
purpose
```

This prevents ciphertext swapping across contexts.

---

# 48. Nonce Safety

Nonce requirements depend on algorithm.

Aequora should use a vetted AEAD abstraction and avoid custom nonce schemes.

---

# 49. Field-Level Encryption

Useful for highly sensitive fields.

Examples:

```text
government ID
private note
bank account reference
```

But encryption can break:

```text
server-side indexing
filtering
validation
search
```

Use selectively.

---

# 50. Searchable Encryption

Do not implement custom searchable encryption in v1.

If encrypted field needs equality lookup, consider:

```text
separate keyed blind index
```

only after threat model review.

---

# 51. Blind Index

Conceptually:

```text
HMAC(index_key, normalized_value)
```

for equality search.

Risks:

```text
frequency leakage
dictionary attacks
normalization complexity
```

Not a default feature.

---

# 52. Snapshot Encryption

Large snapshots may be encrypted before object-storage upload.

Pattern:

```text
snapshot DEK
↓
encrypt chunks
↓
wrap DEK for tenant/snapshot
```

---

# 53. Chunk Encryption

Each chunk should use distinct nonce/context.

Manifest records:

```text
encryption algorithm
key reference
nonce/context metadata
ciphertext hash
plaintext canonical hash where needed
```

---

# 54. Hashing Encrypted Chunks

Maintain distinction:

```text
ciphertext transport hash
canonical plaintext digest
```

Ciphertext hash verifies storage/transfer bytes.

Plaintext canonical digest verifies semantic snapshot content after decryption.

---

# 55. Snapshot Access Revocation

Revoking signed URLs is not enough if chunk already downloaded.

Encryption key revocation/rotation gives stronger control over future decryptability where key architecture allows.

---

# 56. Export Encryption

Exports should support mandatory encryption for sensitive datasets.

Options:

```text
tenant-managed public key
one-time export passphrase-derived key
organization KMS
recipient public key
```

---

# 57. Passphrase-Derived Export Keys

If supported, use a modern password KDF such as:

```text
Argon2id
```

with strong parameters and random salt.

Do not derive directly with SHA-256.

---

# 58. Replay Bundle Encryption

Replay bundles can contain sensitive pre-state.

Treat like exports:

```text
encrypted at rest
short retention
access-controlled
```

---

# 59. Audit Archive Encryption

Cold audit archives may use:

```text
tenant/archive DEK
```

while checkpoints remain separately signed.

---

# 60. Governance and Cryptographic Erasure

Part 14 can destroy:

```text
tenant/subject-specific encryption key
```

to make retained ciphertext unreadable.

---

# 61. Cryptographic Erasure Preconditions

Only claim erasure if:

```text
all relevant plaintext caches removed
all decrypting key copies destroyed
backups do not retain usable key
shared encryption domain does not include retained subjects
```

---

# 62. Key Scope Design

For feasible erasure, avoid one global key encrypting all tenants.

At minimum:

```text
per tenant
```

For highly regulated data, possibly:

```text
per subject
per archive
per export
```

---

# 63. Key Rotation vs Re-Encryption

Two approaches:

```text
rewrap DEKs under new KEK
full data re-encryption
```

Prefer rewrapping when only KEK rotates.

---

# 64. DEK Rotation

Rotate DEK when:

```text
data compromise
algorithm migration
strict policy
```

This requires re-encryption.

---

# 65. Key Versioning

Never overwrite key identity.

New key:

```text
new KeyId
```

Old encrypted data retains reference to old key until migrated.

---

# 66. Crypto Algorithm Versioning

Define stable enums:

```rust
pub enum DigestAlgorithm {
    Blake3V1,
}

pub enum SignatureAlgorithm {
    Ed25519V1,
}

pub enum EncryptionAlgorithm {
    Aes256GcmV1,
    XChaCha20Poly1305V1,
}
```

---

# 67. Algorithm Agility

Protocol/artifact format should include algorithm ID.

This allows migration without ambiguity.

---

# 68. Do Not Build "Algorithm Negotiation" Too Loosely

Avoid downgrade attacks.

Server/admin policy defines allowed algorithms.

Client can advertise support, but server chooses from approved set.

---

# 69. Minimum Crypto Policy

Define:

```rust
pub struct CryptoPolicy {
    pub allowed_digests: Vec<DigestAlgorithm>,
    pub allowed_signatures: Vec<SignatureAlgorithm>,
    pub allowed_encryption: Vec<EncryptionAlgorithm>,
    pub require_signed_snapshots: bool,
    pub require_signed_exports: bool,
}
```

---

# 70. CryptoPolicyVersion

```rust
pub struct CryptoPolicyVersion(u32);
```

Persist in manifests/audit where relevant.

---

# 71. Signed Snapshot Manifest

Flow:

```text
canonical manifest
↓
BLAKE3
↓
server signing key
↓
SignatureEnvelope
```

Client:

```text
verify trusted key
↓
verify signature
↓
verify chunk hashes
↓
install
```

---

# 72. Signed Export Manifest

Same pattern.

This proves:

```text
artifact manifest was issued by authority
```

not necessarily that recipient retained it securely.

---

# 73. Signed Audit Checkpoint

Part 13 audit chain:

```text
root through AuditSequence N
↓
sign root
↓
optionally external anchor
```

---

# 74. External Anchor

Store signed checkpoint in separate trust domain.

Examples:

```text
WORM object storage
security team's archive
independent service
```

---

# 75. Cross-Signing During Rotation

During server key rotation, optionally sign a key-transition statement:

```text
old key attests new key
```

and/or:

```text
new key registry signed by offline root
```

This improves continuity.

---

# 76. Offline Root Key

High-assurance deployment may have:

```text
offline root signing key
↓
signs online server signing keys
```

Online keys sign artifacts.

This limits blast radius.

---

# 77. RootKeyId

```rust
pub struct RootKeyId(Uuid);
```

---

# 78. Key Certificate

Not necessarily X.509.

Aequora can define a small signed key authorization object:

```text
key ID
purpose
public key
validity
issuer
signature
```

---

# 79. Avoid Custom PKI Unless Needed

For ordinary SaaS deployments:

```text
managed key registry + application trust
```

is enough.

Offline-root hierarchy should remain optional enterprise mode.

---

# 80. Optional End-to-End Encryption

Some Aequora applications may want payloads unreadable by server.

Examples:

```text
private messaging
sealed personal notes
certain document blobs
```

This is possible only for data where the server does not need plaintext to enforce domain invariants.

---

# 81. E2E Compatibility Rule

Before enabling E2E for a field/entity, classify server responsibilities.

If server needs plaintext for:

```text
validation
conflict merge
search
scope filtering
analytics
authorization based on content
```

opaque E2E encryption may be incompatible.

---

# 82. E2E Suitable Data

Good candidates:

```text
message body
sealed note
private attachment
client-only secret
```

where server can treat payload as opaque bytes.

---

# 83. E2E Unsuitable Data

Bad candidates:

```text
invoice amount
payment value
permission role
attendance status
workflow state
```

because server must validate them.

---

# 84. Split Entity Pattern

Entity can separate:

```text
server-visible metadata
+
E2E ciphertext payload
```

Example:

```text
Message {
    conversation_id,
    sender_id,
    sent_at,
    ciphertext,
}
```

Server validates metadata, stores opaque ciphertext.

---

# 85. E2E PayloadEnvelope

Conceptually:

```rust
pub struct ProtectedPayload {
    pub scheme: E2eScheme,
    pub key_epoch: E2eKeyEpoch,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}
```

---

# 86. E2E Key Ownership

Keys belong to participants/clients, not server.

Server may store:

```text
encrypted key envelopes
public keys
device key metadata
```

but should not hold plaintext content keys in strict E2E mode.

---

# 87. Group E2E Complexity

For groups, key management becomes a separate subsystem:

```text
membership changes
device addition
device removal
forward secrecy
key rotation
message key distribution
```

Aequora should not invent a messaging ratchet inside generic sync core.

---

# 88. Recommendation for Messaging

If using Aequora-like synchronization for messaging, use a mature E2E protocol/library at the application layer.

Aequora transports opaque protected payloads and metadata.

---

# 89. E2E Conflict Handling

Server cannot merge opaque ciphertext meaningfully.

Therefore protected fields usually use:

```text
append-only
whole-value replacement
manual conflict
client-side merge
```

---

# 90. E2E and Anti-Entropy

Part 03 integrity can hash ciphertext.

It proves replicas agree on protected bytes.

It cannot verify semantic plaintext equivalence without keys.

---

# 91. E2E and Audit

Server audit can record:

```text
protected payload changed
digest before/after
actor/device
```

but not plaintext.

---

# 92. E2E and Search

Search must be:

```text
client-side
```

or use a separately designed privacy-preserving index.

Do not promise server full-text search over encrypted payload.

---

# 93. E2E and Scope Projection

Server can scope based only on visible metadata.

Protected payload must not contain fields required for access-control decisions.

---

# 94. E2E and Rebase

Rebase of opaque value must occur on client.

Server cannot field-merge ciphertext.

---

# 95. E2E and Compaction

Client may compact unsent protected operations if semantic class allows.

Server sees only final ciphertext.

---

# 96. E2E Key Epoch

Define:

```rust
pub struct E2eKeyEpoch(u64);
```

Membership/key rotation creates new epoch.

---

# 97. Old Ciphertext

Old data remains decryptable only if clients retain old key epochs according to product policy.

---

# 98. Device Removal

Removing device from E2E group prevents access to future key epochs.

It does not erase plaintext/ciphertext already copied to that device.

---

# 99. Forward Secrecy

Strong messaging forward secrecy requires ratcheting protocols beyond generic sync.

Keep this explicitly outside Aequora core.

---

# 100. Client Local Key Store

Secrets should be stored using platform secure storage where available.

Do not store plaintext private keys in ordinary local DB rows.

---

# 101. Android

Use Android Keystore-backed keys where appropriate.

Aequora Android adapter can expose secure-key capabilities to application.

---

# 102. Desktop

Possible:

```text
OS keyring
TPM-backed key
encrypted key file protected by OS account
```

depending on platform.

---

# 103. Headless Server

Use:

```text
KMS/HSM/secret manager
```

where production risk warrants.

---

# 104. Secret Zero Problem

Every deployment needs a root trust bootstrap.

Examples:

```text
cloud workload identity
mounted secret
TPM identity
operator-provisioned root
```

Document deployment method explicitly.

---

# 105. Key Backup

Signing/encryption keys may need backup.

But backup increases compromise surface.

Policy must define:

```text
which keys recoverable
which keys intentionally nonrecoverable
```

---

# 106. Cryptographic Erasure vs Key Backup

If a key must support cryptographic erasure:

```text
all backup copies must also be destroyable
```

---

# 107. Availability Tradeoff

Losing tenant encryption key can permanently destroy data.

Use:

```text
redundant protected key storage
recovery process
```

for recoverable keys.

---

# 108. Recovery Key

Optional enterprise design:

```text
tenant DEK wrapped by primary KEK
+
separately wrapped by recovery KEK
```

But cryptographic erasure must destroy both wraps/keys.

---

# 109. Key Access Audit

All sensitive key operations should be audited:

```text
key created
rotated
revoked
exported
recovery used
cryptographic erase executed
```

---

# 110. Never Log Key Material

Logs may contain:

```text
key ID
purpose
status
```

never:

```text
private key
DEK
raw secret
```

---

# 111. Memory Hygiene

Rust reduces many memory bugs but does not automatically zero secrets.

Use secret containers that support zeroization where practical.

---

# 112. Zeroization

Sensitive byte buffers:

```text
private keys
DEKs
passphrases
```

should be zeroized on drop when supported.

---

# 113. Debug Formatting

Secret types must not implement revealing `Debug`.

Use:

```text
Secret<T>
Redacted<T>
```

wrappers.

---

# 114. Serialization Guard

Secret key types should not implement generic `Serialize` unless explicitly required.

This prevents accidental logging/config export.

---

# 115. Key IDs Are Not Secrets

They may be logged/audited.

---

# 116. Side-Channel Scope

Aequora should rely on vetted cryptographic implementations.

Do not write custom constant-time primitives.

---

# 117. Randomness

Cryptographic keys/nonces use OS CSPRNG or KMS/HSM generation.

Part 12 deterministic randomness is **not** used for secret cryptographic key generation.

---

# 118. Deterministic Replay Boundary

Replay should capture:

```text
signature/ciphertext digest
key ID
algorithm
```

but should not regenerate sensitive secrets.

---

# 119. Replay of Signed Operation

Replay verifies:

```text
historical signature
```

using stored public key history.

It does not require device private key.

---

# 120. Historical Public Key Retention

Keep retired public keys as long as signed artifacts need verification.

---

# 121. Historical Encryption Keys

Different: old encryption keys must remain only as long as old ciphertext needs decryption.

This creates a retention dependency.

---

# 122. Key Retention Graph

Governance should know:

```text
which ciphertext depends on which key
```

before destroying key.

---

# 123. KeyReference Index

Logical:

```text
key_id
artifact/data class
reference count or retention horizon
```

---

# 124. Key Destruction Safety

Before deleting decrypting key:

```text
verify no retained required ciphertext depends on it
```

unless intentional cryptographic erasure.

---

# 125. Intentional Erasure Marker

Governance records:

```text
KeyDestroyedForErasure
```

with policy/authorization evidence.

---

# 126. Integrity of Local Database

Database page checksums can detect local corruption, but Aequora semantic integrity remains:

```text
canonical digest
anti-entropy
```

Cryptographic signing of every local row is unnecessary.

---

# 127. Local Metadata MAC

Optional for high-risk threat model:

```text
MAC selected local metadata
```

with device key.

But if attacker controls local process/key, benefit is limited.

Not default.

---

# 128. Threat Model Categories

Part 27 will define full security threat model.

Part 15 assumes threats including:

```text
storage corruption
artifact tampering
stolen export
compromised device key
compromised server signing key
unauthorized object storage read
```

---

# 129. Threat: Object Storage Tampering

Mitigation:

```text
chunk hashes
signed manifest
```

Tampered bytes fail verification.

---

# 130. Threat: Object Storage Disclosure

Mitigation:

```text
encrypted chunks
short-lived signed URLs
tenant key isolation
```

---

# 131. Threat: Malicious Client Modifies Operation

If device signatures enabled:

```text
signature fails
```

But authenticated malicious client can still intentionally create bad operation.

Domain validation remains necessary.

---

# 132. Threat: Compromised Server Database

Audit hash chain alone may be rewritable by DB admin.

External signed checkpoint provides stronger detection.

---

# 133. Threat: Compromised Application Server

If online signing key accessible, attacker may sign malicious artifacts.

Mitigations:

```text
KMS/HSM policy
offline root
short-lived online key
audit
separation of duties
```

---

# 134. Key Usage Policy

KMS/HSM can restrict key to:

```text
sign only
decrypt only
specific service identity
```

Use purpose-specific access.

---

# 135. Signature Verification Failure

For snapshot/export/audit checkpoint:

```text
fail closed
```

Do not continue treating artifact as trusted.

---

# 136. Unknown KeyId

Response:

```text
KeyUnknown
```

Client may refresh trusted key registry.

If still unknown:

```text
reject artifact
```

---

# 137. Expired Signing Key

Historical signature may remain valid if artifact was signed within allowed key validity.

Verification policy uses:

```text
signed_at
key validity
revocation state
```

---

# 138. Clock Trust

`signed_at` from artifact alone is not sufficient against compromised signer.

For high assurance, tie signed checkpoint to:

```text
authoritative sequence
external anchor
```

---

# 139. Signed Key Registry

Clients can fetch:

```text
KeyRegistryManifest
```

signed by a longer-lived trusted root.

---

# 140. KeyRegistryManifest

Contains:

```text
public keys
purposes
validity
revocations
generation
```

---

# 141. KeyRegistryGeneration

```rust
pub struct KeyRegistryGeneration(u64);
```

Monotonic.

Clients reject rollback to older registry unless recovery policy allows.

---

# 142. Rollback Protection

Persist highest accepted key-registry generation locally.

Server should not convince client to trust older revoked state.

---

# 143. New Device Bootstrap

Device first trusts:

```text
application-bundled root/public key
```

or authenticated deployment config.

Then fetches current key registry.

---

# 144. Enterprise Self-Hosted Bootstrap

Operator can provision trust root through:

```text
config file
MDM
installer
environment
certificate pin
```

---

# 145. Multi-Tenant Signing

Two models:

```text
shared service signing key
per-tenant signing key
```

---

# 146. Shared Signing Key

Simpler operations.

Tenant separation relies on signed content binding tenant ID.

---

# 147. Per-Tenant Signing Key

Stronger isolation but more key-management overhead.

Useful for high-assurance tenants.

---

# 148. Signed Content Must Bind Tenant

Always include:

```text
TenantId
```

inside signed canonical data for tenant-scoped artifacts.

---

# 149. Cross-Tenant Artifact Swap

Signature verification plus tenant-bound signed manifest should reject swapping Tenant A snapshot into Tenant B context.

---

# 150. Artifact Versioning

Each signed artifact includes:

```text
artifact type
format version
crypto policy version
tenant/scope
content digest
```

---

# 151. ArtifactEnvelope

Conceptually:

```rust
pub struct SignedArtifactManifest<T> {
    pub artifact_type: ArtifactType,
    pub format_version: u16,
    pub tenant_id: TenantId,
    pub content: T,
    pub content_digest: Digest,
    pub signature: SignatureEnvelope,
}
```

---

# 152. Signature Coverage

Sign canonical manifest **excluding the signature field itself**.

---

# 153. Double Hashing

It is acceptable to sign a cryptographic digest of canonical manifest.

Document exact construction.

---

# 154. Artifact Verification API

```rust
pub trait ArtifactVerifier {
    fn verify<T: CanonicalArtifact>(
        &self,
        artifact: &SignedArtifactManifest<T>,
        trust: &TrustContext,
    ) -> Result<(), VerificationError>;
}
```

---

# 155. TrustContext

Contains:

```text
tenant
expected purpose
trusted root/key registry
current crypto policy
```

---

# 156. Crypto Error Taxonomy

```text
DigestMismatch
SignatureInvalid
KeyUnknown
KeyRevoked
AlgorithmDisallowed
DecryptFailed
AssociatedDataMismatch
RegistryRollback
KeyExpired
```

---

# 157. No Generic "CryptoError"

Typed errors improve diagnostics and safe policy decisions.

---

# 158. Protocol vs Artifact Crypto

Do not necessarily sign every HTTPS sync response.

TLS already authenticates transport.

Artifact signatures are valuable for:

```text
durable detached verification
offline verification
cross-system transfer
```

---

# 159. Optional Response Signing

High-assurance/offline relay deployments may enable:

```text
signed sync response checkpoint
```

but not required initially.

---

# 160. Offline Relay Scenario

If sync packages can be moved through USB/air-gapped relay:

```text
artifact signatures become essential
```

Aequora's canonical bundle format can support this later.

---

# 161. Signed Offline Sync Bundle

Potential future:

```text
client operation bundle signed by device
server response bundle signed by authority
```

transported through untrusted medium.

---

# 162. QR/USB Constraints

Keep outside v1 unless real product need.

Architecture remains compatible.

---

# 163. Cryptographic Capability Negotiation

Client capabilities:

```text
signed-snapshot-v1
encrypted-snapshot-v1
device-signature-v1
```

Server requires features according to policy.

---

# 164. Mandatory Policy

If tenant requires signed snapshots and client cannot verify:

```text
client incompatible
```

Do not silently downgrade.

---

# 165. Downgrade Prevention

Capability negotiation transcript can be included in authenticated TLS session semantics, but primary rule is:

```text
server policy defines mandatory minimum
```

not "best mutually supported" if that permits insecure downgrade.

---

# 166. Crypto Configuration

Example RON:

```ron
crypto: (
    policy_version: 3,

    signing: (
        artifacts: true,
        audit_checkpoints: true,
        device_operations: false,
    ),

    encryption: (
        snapshots: true,
        exports: true,
        replay_bundles: true,
        tenant_data: false,
    ),

    algorithms: (
        digest: Blake3V1,
        signature: Ed25519V1,
        encryption: XChaCha20Poly1305V1,
    ),
)
```

---

# 167. Developer Ergonomics

Normal app code should not manipulate raw keys.

Use:

```rust
AequoraCrypto::builder()
    .key_provider(provider)
    .policy(policy)
```

---

# 168. KeyProvider Implementations

Potential crates:

```text
aequora-keyring-local
aequora-kms-aws
aequora-kms-azure
aequora-kms-gcp
aequora-hsm-pkcs11
aequora-android-keystore
```

Only add providers demanded by deployments.

---

# 169. Core Crypto Crate

Suggested:

```text
aequora-crypto/
├── digest.rs
├── canonical.rs
├── signature.rs
├── encryption.rs
├── key_id.rs
├── policy.rs
├── artifact.rs
├── trust.rs
└── errors.rs
```

---

# 170. Device Crypto Crate

```text
aequora-device-crypto/
├── identity.rs
├── registration.rs
├── signing.rs
├── rotation.rs
└── revocation.rs
```

---

# 171. Server Key Management

```text
aequora-server/
└── crypto/
    ├── key_registry.rs
    ├── signer.rs
    ├── encryption_service.rs
    └── rotation.rs
```

---

# 172. Governance Integration

Part 14 key destruction:

```text
GovernanceStore
→
CryptoKeyStore
```

must be explicit and auditable.

---

# 173. Audit Integration

Part 13 records:

```text
key created
key activated
key retired
key revoked
key destroyed
checkpoint signed
```

---

# 174. Replay Integration

Part 12 replay bundle stores:

```text
key IDs
signature bytes
digest metadata
```

but never secret keys.

---

# 175. Snapshot Integration

Part 10 flow:

```text
build canonical chunks
↓
hash
↓
optional encrypt
↓
publish
↓
sign manifest
```

Client:

```text
verify manifest signature
↓
download
↓
verify ciphertext hash
↓
decrypt
↓
verify canonical chunk hash
↓
install
```

---

# 176. Export Integration

Part 09/13 export:

```text
canonical data
↓
compress
↓
encrypt
↓
hash
↓
sign manifest
```

---

# 177. Anti-Entropy Integration

Part 03 canonical digests are not signatures.

They detect mismatch.

For hostile tampering protection across untrusted storage, pair digest with trusted signed manifest/checkpoint.

---

# 178. Audit Chain Integration

Part 13:

```text
hash chain
+
signed checkpoints
```

is recommended high-assurance model.

---

# 179. Operation Ledger Integrity

Store semantic operation digest.

If same OperationId arrives with different digest:

```text
IdempotencyViolation
```

Device signature, if enabled, strengthens attribution.

---

# 180. Privacy and Metadata

Encryption does not hide all metadata.

Still visible may include:

```text
tenant
entity IDs
timestamps
payload sizes
scope IDs
```

Do not claim full metadata privacy.

---

# 181. Padding

Payload padding can reduce size leakage but increases bandwidth.

Not a default feature.

---

# 182. E2E Metadata Minimization

For protected domains, keep server-visible metadata minimal but sufficient for routing and authorization.

---

# 183. Backup Encryption

Backups should be encrypted independently from live database storage.

Key lifecycle must align with restore and erasure policy.

---

# 184. Restore Key Availability

Disaster recovery tests must include:

```text
can we recover required keys?
```

A backup without decrypting keys is unusable.

---

# 185. Key Disaster Recovery

Runbooks should cover:

```text
signing key loss
tenant KEK loss
KMS outage
revocation
compromise
```

---

# 186. KMS Outage

If key service unavailable:

```text
signed artifact generation may pause
encrypted field access may fail
```

Do not cache raw keys indefinitely merely to avoid outage unless policy permits.

---

# 187. Graceful Crypto Degradation

Depends on purpose.

Example:

```text
optional snapshot signing unavailable:
    maybe delay snapshot publication

required field decrypt unavailable:
    fail request

audit checkpoint signer unavailable:
    continue canonical audit if policy allows, checkpoint later
```

Policy must be explicit.

---

# 188. Required vs Optional Crypto

Define:

```rust
pub enum CryptoRequirement {
    Required,
    Preferred,
    Disabled,
}
```

per feature.

---

# 189. Fail Closed

For `Required`:

```text
verification/encryption failure
→
operation/artifact rejected
```

---

# 190. Crypto Migration

Changing algorithms requires migration plan.

Example:

```text
Ed25519V1
→
future signature algorithm
```

Old artifacts remain verifiable with old algorithm/key registry.

New artifacts use new algorithm.

---

# 191. Dual-Signing Window

For major algorithm migration:

```text
sign with old + new
```

for limited window if ecosystem compatibility requires.

---

# 192. Dual Encryption

Avoid encrypting same data under multiple full ciphertext copies unless needed.

Prefer key wrapping for multi-recipient access.

---

# 193. Multi-Recipient Envelope

One DEK can be wrapped separately for authorized recipients/keys.

Useful for:

```text
tenant + recovery key
group recipients
```

---

# 194. Recipient Removal

Removing one wrapped DEK prevents future access only if recipient did not already retain plaintext/DEK.

---

# 195. Key Escrow

If product offers recovery/escrow, state clearly that strict E2E confidentiality is weakened.

Do not call escrowed encryption "server-blind E2E" if server/operator can recover keys.

---

# 196. E2E Modes

Define accurately:

```rust
pub enum PayloadProtectionMode {
    ServerReadable,
    TenantEncryptedAtRest,
    ClientManagedE2E,
}
```

---

# 197. ServerReadable

Server sees plaintext and can fully validate.

---

# 198. TenantEncryptedAtRest

Server application can decrypt using tenant key.

Protects storage, not server operator/runtime.

---

# 199. ClientManagedE2E

Server cannot decrypt payload.

Server validation limited to metadata/ciphertext structure.

---

# 200. No Ambiguous "Encrypted" Label

UI/docs should distinguish these modes.

---

# 201. Testing — Cryptographic Vectors

Maintain deterministic test vectors for:

```text
canonical digest
signature verification
artifact manifest hashing
AAD construction
```

---

# 202. Property Tests

Assert:

```text
changing any signed semantic field invalidates signature
changing tenant context invalidates verification
ciphertext swap across entity context fails AAD authentication
```

---

# 203. Fuzzing

Fuzz:

```text
artifact parser
encrypted envelope parser
key-registry parser
signature envelope
```

---

# 204. Negative Tests

Test:

```text
unknown key
revoked key
wrong tenant
wrong purpose
bad signature
modified manifest
truncated ciphertext
nonce corruption
wrong AAD
```

---

# 205. Rotation Tests

Scenario:

```text
artifact A signed with key K1
rotate to K2
artifact B signed with K2
K1 VerificationOnly
```

Expected:

```text
A verifies
B verifies
new signing uses K2
```

---

# 206. Revocation Tests

K1 compromised/revoked.

Verification policy should surface:

```text
revoked-key status
```

not silently treat as ordinary valid.

---

# 207. Key Registry Rollback Test

Client accepted generation 10.

Server/middlebox presents generation 9.

Expected:

```text
reject rollback
```

unless explicit recovery flow.

---

# 208. Snapshot Tamper Test

Modify one chunk.

Expected:

```text
chunk hash mismatch
bootstrap stops
```

---

# 209. Manifest Tamper Test

Modify chunk list after signing.

Expected:

```text
signature invalid
```

---

# 210. Cross-Tenant Swap Test

Use Tenant A encrypted/signed artifact in Tenant B context.

Expected:

```text
AAD/signature trust mismatch
```

---

# 211. Device Signature Test

Modify payload after device signing.

Expected:

```text
signature verification fails
```

---

# 212. E2E Test

Server stores ciphertext.

Another authorized client with key decrypts.

Server-side semantic validator must not pretend it can inspect protected plaintext.

---

# 213. Key Loss Test

Destroy tenant key in test.

Expected:

```text
ciphertext no longer decryptable
governance state records key destruction
```

---

# 214. Restore Test

Restore encrypted backup plus key material.

Verify:

```text
data decrypts
signed checkpoints verify
key registry restored correctly
```

---

# 215. Formal Invariants

Add:

## AEQ-INV-CRYPTO001

```text
A signed artifact is accepted only if its canonical digest verifies under a trusted key authorized for the artifact purpose.
```

## AEQ-INV-CRYPTO002

```text
Tenant-scoped signed/encrypted artifacts cryptographically bind the tenant context.
```

## AEQ-INV-CRYPTO003

```text
Key rotation never requires changing historical artifact signatures.
```

## AEQ-INV-CRYPTO004

```text
A revoked key is never used for new signatures or encryption.
```

## AEQ-INV-CRYPTO005

```text
Secrets and private key material are never serialized into ordinary logs, manifests, or replay bundles.
```

## AEQ-INV-CRYPTO006

```text
Required cryptographic verification fails closed.
```

---

# 216. Additional Invariants

## AEQ-INV-CRYPTO007

```text
Opaque end-to-end protected payloads are never used for server-side domain decisions requiring plaintext semantics.
```

## AEQ-INV-CRYPTO008

```text
Cryptographic erasure is not reported complete while any known retained usable decryption key copy remains.
```

## AEQ-INV-CRYPTO009

```text
The same OperationId that may have reached authority cannot later be paired with a different signed semantic payload.
```

---

# 217. Observability

Metrics:

```text
crypto_signature_verify_failure_total
crypto_decrypt_failure_total
crypto_key_rotation_total
crypto_key_revocation_total
crypto_registry_generation
crypto_artifact_verify_total
```

Do not label with key IDs unless operationally necessary and cardinality controlled.

---

# 218. Logs

Structured events:

```text
key_rotated
key_revoked
artifact_signature_failed
key_registry_updated
crypto_policy_rejected_algorithm
```

Never log secret material.

---

# 219. Alerting

Alert on:

```text
signature verification spike
decrypt failure spike
unknown signing key
key registry rollback attempt
expired active key
KMS unavailable
```

---

# 220. Admin CLI

Suggested:

```text
aequora crypto key list
aequora crypto key rotate
aequora crypto key revoke
aequora crypto registry verify
aequora crypto artifact verify
aequora crypto checkpoint verify
aequora crypto policy show
```

Destructive key operations require strong authorization.

---

# 221. Separation of Duties

For high-assurance deployments:

```text
application admin
security/key admin
audit reviewer
```

can be separate roles.

---

# 222. Key Rotation Automation

Routine rotation can be scheduled.

Compromise revocation remains explicit security action.

---

# 223. Key Expiry Monitoring

Warn before active signing/encryption key reaches expiry.

---

# 224. Startup Validation

Server startup should verify:

```text
required active keys exist
key purposes match
algorithms allowed
key registry valid
```

---

# 225. Client Startup Validation

Client verifies:

```text
trusted root available
stored registry generation valid
device key accessible if required
```

---

# 226. Plug-and-Play Defaults

Recommended default Aequora deployment:

```text
TLS required
BLAKE3 artifact hashes
server-signed snapshot/export/audit checkpoint optional-but-supported
device operation signatures disabled unless needed
tenant application-level encryption optional
E2E payloads opt-in per domain
```

---

# 227. Why Not Enable Everything By Default

Maximum cryptographic layering can introduce:

```text
operational fragility
key-loss risk
development complexity
performance cost
```

Use stronger modes when threat model justifies them.

---

# 228. Security Profiles

Possible presets:

```rust
CryptoProfile::Standard
CryptoProfile::Enterprise
CryptoProfile::HighAssurance
CryptoProfile::ClientManagedE2E
```

---

# 229. Standard

```text
TLS
BLAKE3 hashes
storage-provider encryption
```

---

# 230. Enterprise

```text
Standard
+ signed snapshots
+ signed exports
+ signed audit checkpoints
+ tenant-scoped envelope encryption
```

---

# 231. HighAssurance

```text
Enterprise
+ KMS/HSM
+ offline/root key hierarchy
+ device operation signatures
+ external audit anchors
+ strict crypto policy
```

---

# 232. ClientManagedE2E

Applied only to selected payload domains.

Not a whole-system switch.

---

# 233. Compatibility With Pure Rust Goal

Core cryptographic implementation can remain Rust-first.

Platform/KMS adapters may call:

```text
OS APIs
cloud APIs
HSM interfaces
```

through isolated crates.

---

# 234. Dependency Policy

Only use widely reviewed cryptographic crates with suitable licenses.

Avoid obscure unmaintained primitives.

---

# 235. No Homegrown Crypto

Aequora may define:

```text
protocol composition
key hierarchy
domain separation
```

but must not invent:

```text
cipher
signature primitive
KDF
hash algorithm
```

---

# 236. Completion Criteria

Part 15 is complete when:

```text
[ ] cryptographic layers separated
[ ] BLAKE3 canonical hashing standardized
[ ] server signing architecture defined
[ ] key purpose separation defined
[ ] key registry and rotation defined
[ ] device signing optional architecture defined
[ ] tenant envelope encryption defined
[ ] snapshot/export/replay encryption defined
[ ] audit checkpoint signing defined
[ ] key revocation and registry rollback protection defined
[ ] cryptographic erasure integration defined
[ ] E2E compatibility rules defined
[ ] protected payload envelope defined
[ ] KMS/HSM/provider abstraction defined
[ ] secret handling/zeroization policy defined
[ ] testing/fuzzing/negative vectors defined
[ ] cryptographic invariants added
```

---

# 237. Final Architecture

```text
                         TRUST ROOT
                             │
                             ▼
                       Key Registry
                             │
            ┌────────────────┼────────────────┐
            ▼                ▼                ▼
      Server Signing    Tenant KEK      Device Public Keys
            │                │                │
            ▼                ▼                ▼
   Signed Manifests     Wrapped DEKs     Signed Operations
            │                │                │
            ▼                ▼                ▼
 Snapshot / Export      Encrypted Data     Auth + Verify
 Audit Checkpoint            │                │
            │                ▼                ▼
            │          Storage / Blobs   Domain Validation
            │
            ▼
      Offline Verification

Optional E2E domain:

 Client A
    │
    ▼
Client-Managed Key
    │
    ▼
Opaque Ciphertext
    │
    ▼
Aequora Server
(metadata validation only)
    │
    ▼
Client B
    │
    ▼
Decrypt + Client-Side Semantics
```

The architectural principle is:

> **Aequora should cryptographically protect the boundaries that need independent proof—artifacts, keys, tenant data, device identity, and selected opaque payloads—while keeping domain authority explicit about what the server can and cannot validate.**

This gives Aequora strong integrity and confidentiality without turning generic synchronization into an opaque cryptographic system that undermines conflict handling, authorization, auditability, or operational recovery.
