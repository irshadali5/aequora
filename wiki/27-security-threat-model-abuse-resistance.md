# Aequora Sync — Part 27

# Dedicated Security Threat Model and Abuse Resistance Architecture

## 1. Purpose

Aequora is a synchronization engine that can sit at the center of:

```text
school ERP
finance/accounting
healthcare
enterprise workflow
document systems
payments
multi-device local-first applications
```

That means it processes:

```text
identity
authorization
sensitive business data
offline operations
audit history
background jobs
cryptographic keys
snapshots
imports
exports
webhooks
```

Security cannot be treated as a set of scattered implementation notes.

Aequora needs one dedicated threat model that identifies:

```text
assets
attackers
trust boundaries
entry points
abuse cases
security invariants
mitigations
verification strategies
```

The central rule is:

> **Aequora must assume every network client, every external payload, every stale device, every compatibility request, and every integration boundary is untrusted until proven otherwise.**

---

# 2. Security Goals

The security architecture should protect:

```text
confidentiality
integrity
authenticity
authorization
availability
tenant isolation
auditability
replay resistance
rollback resistance
operational control integrity
```

---

# 3. Security Non-Goals

Aequora does not guarantee:

```text
a compromised operating system cannot read plaintext already available to the application
perfect prevention of malicious authorized administrators
absolute availability against unlimited DDoS
forward secrecy for custom E2E payloads unless application protocol provides it
```

Threat boundaries must remain realistic.

---

# 4. Security Principles

Aequora follows:

```text
deny by default
least privilege
explicit trust
bounded input
fail closed
defense in depth
strong identity
immutable audit evidence
separation of duties
minimal secrets
```

---

# 5. Threat Modeling Method

Use a pragmatic hybrid:

```text
asset inventory
trust-boundary diagram
STRIDE-style threat enumeration
abuse-case testing
security invariants
```

---

# 6. Assets

Critical assets include:

```text
authoritative business state
operation ledger
journal
audit trail
tenant data
user/device identity
authorization policy
cryptographic keys
authority epoch metadata
legal holds
payment/provider references
admin control plane
```

---

# 7. Attacker Classes

Model at least:

```text
Unauthenticated Internet Attacker
Authenticated Malicious User
Compromised Client Device
Malicious Tenant Administrator
Compromised Application Node
Malicious Insider
Compromised External Provider
Network Attacker
Supply-Chain Attacker
Resource-Exhaustion Attacker
```

---

# 8. Trust Boundaries

Primary boundaries:

```text
Client ↔ Internet
Internet ↔ Axum Data Plane
Admin ↔ Control Plane
Server ↔ PostgreSQL
Server ↔ Object Storage
Server ↔ KMS/Secret Store
Server ↔ External Provider
Worker ↔ External Provider
Replica ↔ Authority
Legacy Bridge ↔ Legacy System
```

---

# 9. Client Is Untrusted

Even official client software can be modified.

Therefore never trust client-provided:

```text
tenant
actor
role
priority
scope
authority
version
timestamp
permissions
```

without server validation.

---

# 10. Server Authentication

Server authenticates:

```text
user
service
device
```

before protected operations.

---

# 11. Authentication Context

Use:

```rust
pub struct AuthContext {
    pub principal_id: PrincipalId,
    pub tenant_id: TenantId,
    pub device_id: Option<DeviceId>,
    pub auth_method: AuthMethod,
    pub assurance: AssuranceLevel,
}
```

---

# 12. Tenant Binding

TenantId in operation payload is not trusted.

Server derives/validates tenant membership from authenticated identity.

---

# 13. Cross-Tenant Attack

Attacker submits:

```text
their token
+
victim TenantId
```

Expected:

```text
authorization failure
```

before business data lookup where possible.

---

# 14. Entity Enumeration

IDs such as UUIDv7 are hard to guess but not authorization.

Even known EntityId requires permission.

---

# 15. Scope Enumeration

ScopeId is not capability.

Server revalidates scope authorization.

---

# 16. Device Identity

Device key proves possession of a device key.

It does not prove:

```text
human intent
authorization
tenant membership
```

---

# 17. Device Revocation

Revoked device:

```text
cannot authenticate new sync
cannot submit operations
```

Pending local data may remain on device until purge/encryption policy handles it.

---

# 18. Replay Attack

Attacker resends captured valid operation.

Aequora mitigates via:

```text
OperationId idempotency
device/auth validation
payload digest
ledger
```

---

# 19. Replay Across Tenant

Same signed payload must not be valid under another tenant.

Bind:

```text
TenantId
operation semantics
device identity
```

appropriately.

---

# 20. Replay Across Authority

An operation may be retried across valid authority failover.

Do not bind too tightly unless security requires.

Authority still reauthenticates/revalidates.

---

# 21. Payload Substitution

Same OperationId + changed payload:

```text
reject
```

via semantic payload digest.

---

# 22. Message Tampering

TLS protects transit.

For durable/offline artifacts use:

```text
BLAKE3
signatures
```

where Part 15 specifies.

---

# 23. TLS Requirements

Production:

```text
TLS 1.2+ minimum
prefer TLS 1.3
valid certificate verification
hostname verification
```

Actual deployment policy may require stronger.

---

# 24. Certificate Pinning

Not required by default.

Operational rotation complexity may outweigh benefit.

Use only where threat model justifies.

---

# 25. MITM

Normal public PKI/TLS protects.

High-assurance deployments may add:

```text
mTLS
private CA
```

---

# 26. Protocol Downgrade

Attacker attempts weaker version/capability.

Part 21 server policy ensures:

```text
required minimum
required crypto/security features
```

---

# 27. Security Capability

Security-sensitive capability marked:

```text
RequiredForSafety
```

never silently omitted.

---

# 28. Unknown Protocol Input

Reject unknown required fields/message kinds safely.

---

# 29. Parser Security

All wire parsers enforce:

```text
max frame size
max vector length
max nesting
max dependency edges
```

---

# 30. Deserialization Bomb

Compact binary formats can still create allocation attacks.

Bound before/while decode.

---

# 31. Integer Overflow

Use checked arithmetic for:

```text
length
offset
sequence math
capacity
```

---

# 32. UTF-8

Validate at boundary.

---

# 33. Unicode Confusables

Human identifiers may have spoofing risk.

For security principal names, display stable IDs/context.

Do not use display name for authorization.

---

# 34. Authentication Brute Force

Mitigate with:

```text
rate limiting
lockout policy where appropriate
MFA for admin
```

---

# 35. Token Theft

Use:

```text
short-lived access tokens
secure client storage
refresh-token protections
device revocation
```

---

# 36. Token Logging

Never log bearer token.

---

# 37. Cookie Security

Browser surfaces:

```text
Secure
HttpOnly
SameSite
CSRF protection
```

where cookie auth used.

---

# 38. CORS

Restrictive explicit origin policy.

---

# 39. CSRF

Relevant for browser cookie-authenticated admin/data APIs.

---

# 40. Authorization Model

Authorization must be centralized enough that:

```text
sync
REST
admin
jobs
```

cannot diverge silently.

---

# 41. Domain Authorization

Recommended:

```text
authenticate
↓
authorize operation
↓
domain validate
↓
execute
```

---

# 42. Object-Level Authorization

Every entity operation checks access to referenced aggregate.

---

# 43. Field-Level Authorization

Some fields may require stronger permission.

Example:

```text
salary
bank details
security role
```

---

# 44. Mass Assignment

Never deserialize arbitrary client fields directly into DB update model.

Use typed operation payload.

---

# 45. Privilege Escalation

Operations modifying:

```text
roles
permissions
device state
```

need explicit high-assurance handlers.

---

# 46. Authorization TOCTOU

Authorization and write should be sufficiently coupled.

If mutable permissions can change concurrently, validate within authoritative execution context/transaction as required.

---

# 47. Stale Authorization Cache

Cache must include:

```text
policy generation
revocation
TTL
```

---

# 48. Admin Security

Part 24 control plane should be isolated.

Require:

```text
strong auth
least privilege
audit
approval
```

---

# 49. Break-Glass

Break-glass access:

```text
short-lived
critical alert
full audit
strong authentication
```

---

# 50. Admin Idempotency

AdminOperationId prevents accidental duplicate destructive actions.

---

# 51. Insider Threat

Malicious privileged admin may still harm system.

Mitigate with:

```text
separation of duties
two-person approval
tamper-evident audit
external audit checkpoints
least privilege
```

---

# 52. Direct Database Access

Human direct DB write bypasses application invariants.

Production should limit write access.

---

# 53. DB Role Separation

Potential roles:

```text
app runtime
migration
read-only support
backup
```

---

# 54. Row-Level Security

Optional PostgreSQL defense in depth for tenant isolation.

---

# 55. SQL Injection

SQLx parameterized queries.

Never concatenate untrusted SQL fragments.

---

# 56. Dynamic Filter Injection

Part 07 does not expose raw SQL filters from client.

Typed scope parameters only.

---

# 57. Legacy SQL

Legacy bridge must parameterize.

---

# 58. SSRF

Webhook/external URL features can target internal services.

Mitigate:

```text
scheme allowlist
DNS/IP validation
private network block
redirect validation
egress policy
```

---

# 59. DNS Rebinding

Validate resolved IP at connection time, not only hostname string.

---

# 60. Redirect SSRF

Revalidate every redirect target or disable redirects.

---

# 61. Cloud Metadata

Block access to:

```text
169.254.169.254
```

and equivalent metadata endpoints.

---

# 62. Webhook Authentication

Outgoing webhook can use:

```text
HMAC/signature
timestamp
delivery ID
```

---

# 63. Webhook Replay

Receiver can dedupe using DeliveryId and verify timestamp/signature.

---

# 64. Email Header Injection

Validate addresses/headers.

Do not concatenate raw newline-containing values.

---

# 65. HTML Email

Escape untrusted content or use trusted templates.

---

# 66. Payment Security

Payment operations require:

```text
server-side amount calculation
idempotency key
provider reference verification
webhook signature verification
```

---

# 67. Client Amount Is Untrusted

Never charge based solely on:

```text
client-provided total
```

Server recomputes authoritative amount.

---

# 68. Payment Webhook

Verify provider signature and event ID.

---

# 69. Duplicate Provider Event

Deduplicate.

---

# 70. Provider Impersonation

Do not trust source IP alone.

Use signature/secret verification.

---

# 71. Background Job Injection

Only registered typed job creation paths.

No arbitrary JobKind/payload API.

---

# 72. Job Privilege

Worker gets least privilege needed.

---

# 73. Stale Worker

Part 23 fencing prevents stale worker DB checkpoint.

---

# 74. External Side Effect Duplication

Use idempotency/reconciliation.

---

# 75. Snapshot Security

Threats:

```text
tampered chunk
stale snapshot
cross-tenant snapshot
unauthorized download
```

---

# 76. Snapshot Mitigation

Use:

```text
signed manifest
chunk digests
tenant binding
AuthorityEpoch
short-lived URLs
```

---

# 77. Snapshot URL Leakage

Signed URL should expire quickly.

Artifact encryption can reduce exposure.

---

# 78. Object Storage ACL

Private bucket/container.

No public listing.

---

# 79. Cross-Tenant Object Key

Object namespace includes tenant/opaque identifiers.

Do not rely only on path for authorization.

---

# 80. Blob Upload

Threats:

```text
oversized file
malware
content-type spoof
path traversal
```

---

# 81. Blob Limits

Enforce:

```text
size
chunk count
quota
```

---

# 82. Malware Scanning

Application/domain optional depending product.

Not core sync requirement.

---

# 83. File Name

Treat as display metadata.

Never use untrusted filename as filesystem path.

---

# 84. Archive Import

Part 09/25 archive parser must block:

```text
path traversal
symlink escape
archive bomb
```

---

# 85. Import Security

Imported records are untrusted data.

Validate all domain invariants.

---

# 86. CSV Formula Injection

If exporting CSV for spreadsheet use, escape formula-leading fields where needed.

---

# 87. Export Authorization

Exports often contain broad data.

Require:

```text
explicit permission
audit
expiry
encryption
```

---

# 88. Export Link

Short-lived and authenticated.

---

# 89. Audit Security

Audit should be:

```text
append-only
tamper-evident
access-controlled
```

---

# 90. Audit PII

Do not overcollect sensitive values.

---

# 91. Audit Tampering

Hash chain/checkpoints can reveal mutation.

---

# 92. Audit Deletion

Governance policy controls.

Do not let ordinary admin delete individual audit rows.

---

# 93. Authority Security

Part 16 protects against:

```text
split brain
rollback
stale primary
```

---

# 94. Authority Epoch Rollback

Client/store should remember highest trusted epoch.

Lower epoch:

```text
fail
```

---

# 95. Authority Registry

High-assurance deployments may externally anchor latest epoch.

---

# 96. Split Brain

Security + correctness risk.

Mitigation:

```text
fencing
single writer
storage/infrastructure role
```

---

# 97. Replica Poisoning

Read replica must match:

```text
AuthorityId
AuthorityEpoch
```

and trusted replication source.

---

# 98. Regional Routing Attack

Client should not choose arbitrary server as authority.

Use trusted endpoint/control plane.

---

# 99. DNS Poisoning

TLS hostname verification protects endpoint authenticity.

---

# 100. Local Client Threat

A compromised local device may:

```text
read cached data
modify local DB
forge local UI
steal tokens
```

Server must remain secure anyway.

---

# 101. Local DB Tampering

Client local state is not authoritative.

On sync, server validates operation.

Anti-entropy can detect replica divergence.

---

# 102. Local Outbox Tampering

Attacker can modify local pending operation.

If device signature used:

```text
signature detects
```

Otherwise server still treats operation as authenticated client input.

---

# 103. Device Signature Benefit

Useful against:

```text
local storage tampering after signing
transport substitution
```

not compromised active application with key access.

---

# 104. Rooted/Jailbroken Device

Do not assume secrecy.

Can increase risk score or restrict sensitive offline data according to app policy.

---

# 105. Secure Storage

Use:

```text
Android Keystore
iOS Keychain/Secure Enclave where available
OS credential store
```

---

# 106. Desktop Secret Storage

Prefer:

```text
Secret Service
Windows Credential Manager
macOS Keychain
```

through platform adapter.

---

# 107. Local Data Encryption

Application may encrypt local database/file at rest.

Aequora can integrate but should not invent weak crypto.

---

# 108. Passcode-Derived Keys

If used, use:

```text
Argon2id
```

with appropriate parameters.

---

# 109. Memory Secrets

Avoid:

```text
Debug
Clone
Serialize
```

on secret types unless necessary.

---

# 110. Zeroization

Use vetted zeroization for secret buffers where meaningful.

---

# 111. Clipboard

Sensitive values should not be copied automatically.

UI concern.

---

# 112. Screenshots

Mobile app may restrict screenshots for highly sensitive screens if product requires.

---

# 113. Logs

Security logging rules:

```text
no secrets
no raw auth tokens
minimal PII
stable error codes
```

---

# 114. Error Messages

Do not expose:

```text
SQL details
stack traces
internal paths
secret config
```

to untrusted clients.

---

# 115. User Enumeration

Login/reset errors should avoid unnecessary account existence leakage where applicable.

---

# 116. Rate Limiting

Part 18 protects:

```text
brute force
request floods
expensive API abuse
```

---

# 117. Cost Amplification Attack

Small request causing huge DB/CPU work.

Mitigate:

```text
bounded DAG
bounded scope complexity
cost-based admission
```

---

# 118. Reconnect Storm as Abuse

Same mechanisms handle both accidental and malicious storms.

---

# 119. Tenant Noisy Neighbor

Per-tenant limits preserve availability.

---

# 120. Slowloris

HTTP server/load balancer should enforce:

```text
header timeout
body timeout
minimum throughput where suitable
connection limits
```

---

# 121. Large Body Attack

Content-Length/pre-decode bounds.

Streaming body limit.

---

# 122. Compression Bomb

Bound:

```text
compressed bytes
decompressed bytes
ratio
```

---

# 123. Hash Flooding

Use robust hasher for attacker-controlled map keys.

---

# 124. Regex DoS

Avoid unbounded user-supplied regex in scope/search.

---

# 125. Search Abuse

Limit:

```text
query length
filters
result size
```

---

# 126. Pagination

All list APIs bounded.

---

# 127. Cursor Abuse

Server validates cursor structure/epoch/scope ownership.

---

# 128. Fake High Cursor

Client cannot force server retention by claiming arbitrary state.

Device watermark update only after validated session.

---

# 129. Fake Low Cursor

May force expensive replay.

Retention floor/rebootstrap and admission protect.

---

# 130. Scope Explosion

Limit:

```text
active scopes per device
subscriptions per connection
```

---

# 131. Snapshot Herd Attack

Admission + auth + CDN + quotas.

---

# 132. Job Explosion

Part 23 fan-out limits.

---

# 133. Admin Endpoint DoS

Admin also rate-limited.

Network-private does not mean safe.

---

# 134. Supply-Chain Threat

Rust crates/build dependencies may be compromised.

Mitigate:

```text
lockfile
dependency review
license policy
cargo-deny
cargo-audit
SBOM
pinned CI
```

---

# 135. Permissive Dependency Policy

Aequora prefers vetted permissive dependencies.

Security review remains independent of license.

---

# 136. Minimal Dependencies

Do not add heavy dependency for tiny convenience.

Reduces attack surface.

---

# 137. Feature Gating

Compile only required providers/adapters.

---

# 138. Unsafe Code

Part 19 default:

```text
forbid unsafe in core
```

---

# 139. Unsafe Dependency

Dependencies may contain unsafe.

Review high-risk crates.

---

# 140. Build Scripts

`build.rs` executes code at build time.

Review/pin dependencies with build scripts.

---

# 141. Proc Macros

Also executable at build time.

Treat as supply-chain code.

---

# 142. Reproducible Builds

Helpful for verifying release artifacts.

---

# 143. Signed Release Artifacts

Part 15 can sign:

```text
server binaries
client installers
metadata manifests
```

application distribution concern.

---

# 144. SBOM

Generate:

```text
CycloneDX/SPDX
```

if enterprise/compliance needs.

---

# 145. Vulnerability Scanning

CI checks known advisories.

---

# 146. Dependency Update Policy

Regular, controlled, tested.

Avoid both:

```text
never update
```

and:

```text
blind auto-merge security-sensitive changes
```

---

# 147. Compiler/Toolchain

Pin Rust toolchain in CI.

---

# 148. Hermetic Build

Where practical, use locked toolchain/dependencies.

---

# 149. CI Secret Security

Secrets only in protected pipelines.

Avoid printing.

---

# 150. Artifact Provenance

Record:

```text
build identity
commit
toolchain
lockfile digest
```

---

# 151. Dependency Confusion

Use crates.io/pinned registry policy.

Private crates use distinct naming/registry controls.

---

# 152. Typosquatting

Dependency review and lockfile.

---

# 153. Server Host Security

Aequora cannot replace OS hardening.

Recommended:

```text
non-root
read-only filesystem where possible
minimal container
seccomp/AppArmor/SELinux
```

deployment-dependent.

---

# 154. File Permissions

Config/secrets restricted.

---

# 155. Temporary Files

Use secure temp location/permissions.

---

# 156. Crash Dumps

May contain secrets.

Disable or protect in production.

---

# 157. Database Encryption

At-rest disk/storage encryption recommended.

---

# 158. Backup Security

Backups encrypted/access-controlled.

---

# 159. Backup Exfiltration

Backups can contain entire tenant dataset.

Treat as high-value asset.

---

# 160. Restore Security

Restore process requires strong admin permission/audit.

---

# 161. KMS Threat

KMS compromise can expose key usage.

Use:

```text
least privilege
key purpose separation
audit
rotation
```

---

# 162. Key Confusion

Part 15 purpose tags prevent using one key for wrong role.

---

# 163. Nonce Reuse

Use vetted AEAD APIs that safely generate/manage nonces.

---

# 164. Algorithm Downgrade

Crypto policy forbids disallowed algorithms.

---

# 165. Cross-Tenant Encryption

AAD includes TenantId.

---

# 166. Cryptographic Erasure

Do not claim success if recoverable key copies remain.

---

# 167. E2E Threat Model

Client-managed E2E protects plaintext from server.

It does not protect:

```text
metadata
traffic timing
compromised recipient device
```

---

# 168. E2E Server Validation

Server cannot enforce semantics on encrypted fields.

Only use where domain allows opaque payload.

---

# 169. Key Recovery

Strict E2E key loss may mean permanent data loss.

Make product behavior explicit.

---

# 170. Metadata Leakage

Even encrypted content may reveal:

```text
sender
recipient
size
time
```

unless additional protocol addresses it.

---

# 171. Legacy Bridge Threat

Legacy DB may be compromised or dirty.

Treat CDC as untrusted input.

---

# 172. Legacy Write After Cutover

Critical integrity/security incident.

Fence and alert.

---

# 173. Import File Threat

Untrusted file parser isolated/bounded.

---

# 174. CSV/Spreadsheet Formula

Escape exports where relevant.

---

# 175. Audit Export Threat

Exports may be abused for bulk exfiltration.

Require elevated permission and audit.

---

# 176. Data Exfiltration via Search

Rate limits and authorization apply.

---

# 177. Enumeration via Errors

Avoid revealing:

```text
entity exists but forbidden
```

where privacy requires indistinguishable error.

---

# 178. Security Error Policy

Application can choose:

```text
NotFoundOrForbidden
```

for sensitive object lookup.

---

# 179. Tenant Boundary Tests

Every repository/query must be tested against cross-tenant access.

---

# 180. Property-Based Authorization Tests

Generate:

```text
principals
tenants
entities
roles
```

and assert no unauthorized read/write.

---

# 181. Security Invariant Registry

Maintain stable security invariant IDs.

---

# 182. Threat: Operation Forgery

Attack:

```text
malicious client creates impossible state transition
```

Mitigation:

```text
typed operation
authz
domain validation
version check
```

---

# 183. Threat: Version Bypass

Client omits base version.

Only operation profiles that allow no base version may accept.

---

# 184. Threat: LWW Abuse

Client sets fake future timestamp.

Server must not use untrusted wall clock as authority.

---

# 185. Threat: Conflict Suppression

Client cannot mark conflict resolved by editing local state.

Resolution requires authoritative operation.

---

# 186. Threat: Tombstone Resurrection

Stale client sends old entity after deletion.

Mitigation:

```text
generation/floor/retired entity checks
```

---

# 187. Threat: Cursor Forgery

Cannot skip required changes by arbitrary cursor if server/authorization semantics require validated state.

---

# 188. Threat: Scope Filter Injection

Typed filters only.

---

# 189. Threat: Scope Authorization Bypass

Server recomputes resolved scope.

---

# 190. Threat: Snapshot Substitution

Manifest binds:

```text
tenant
scope
epoch
root
signature
```

---

# 191. Threat: Old Snapshot Replay

Epoch/generation check rejects stale artifact.

---

# 192. Threat: Journal Fork

Signed checkpoints/digest mismatch fail closed.

---

# 193. Threat: Authority Rollback

Highest epoch rollback protection.

---

# 194. Threat: Job Replay

Job state/idempotency/fencing.

---

# 195. Threat: Payment Replay

Provider idempotency key.

---

# 196. Threat: Admin Action Replay

AdminOperationId + payload digest.

---

# 197. Threat: Governance Bypass

Purge execution rechecks legal holds/policy.

---

# 198. Threat: Audit Deletion

No ordinary mutation API.

---

# 199. Threat: Compatibility Downgrade

Required capability policy.

---

# 200. Threat: Resource Exhaustion

Part 18.

---

# 201. Threat: Diagnostic Exfiltration

Part 25 sanitization/encryption/authorization.

---

# 202. Threat: Incident Bundle Malware

Bounded parser/no execution.

---

# 203. Threat: Plugin/Provider Abuse

Provider interfaces receive minimal typed data.

No arbitrary code plugins in core by default.

---

# 204. Dynamic Plugin Runtime

Not v1.

Compile-time features reduce runtime code injection risk.

---

# 205. WASM Plugins

If future added, need separate sandbox threat model.

---

# 206. Security Zones

Suggested deployment:

```text
Public Edge
Application Zone
Database Zone
Control Plane Zone
Secrets/KMS Zone
Object Storage Zone
```

---

# 207. Public Edge

Handles:

```text
TLS
rate limiting
request size limits
```

---

# 208. Application Zone

Axum nodes/workers.

---

# 209. Database Zone

Private network.

---

# 210. Control Plane Zone

Restricted/private.

---

# 211. KMS Zone

Only authorized services.

---

# 212. Egress Control

Application workers allowed only required destinations.

---

# 213. Zero Trust Internal

Internal service call should authenticate.

Do not rely solely on subnet.

---

# 214. mTLS Internal

Optional/recommended enterprise.

---

# 215. Service Identity

Use workload identity/certificates where available.

---

# 216. Internal Authorization

Worker service cannot call authority admin endpoints unless permission.

---

# 217. Secret Rotation

Design all long-lived credentials for rotation without downtime.

---

# 218. Credential TTL

Prefer short-lived cloud/workload credentials.

---

# 219. Secret Version

Provider config references:

```text
secret version/key ID
```

for audit.

---

# 220. Security Headers

Browser admin/web surfaces:

```text
CSP
HSTS
X-Content-Type-Options
Referrer-Policy
```

as appropriate.

---

# 221. XSS

Dioxus/web rendering escapes by default where supported.

Never inject raw HTML from untrusted input without sanitization.

---

# 222. Markdown

If rendering user Markdown, sanitize HTML and dangerous links.

---

# 223. URL Schemes

Allowlist:

```text
https
mailto
```

as product requires.

Block:

```text
javascript:
data:
```

where unsafe.

---

# 224. File Download Headers

Use safe:

```text
Content-Type
Content-Disposition
```

and filename encoding.

---

# 225. Content Sniffing

Set `nosniff`.

---

# 226. Session Fixation

Regenerate session on authentication/privilege elevation where cookie sessions used.

---

# 227. Logout

Revoke/expire refresh/session tokens.

---

# 228. MFA

Recommended for:

```text
admin
finance approvals
security operations
```

depending application.

---

# 229. Step-Up Auth

Part 24 assurance level can require fresh MFA for dangerous actions.

---

# 230. Password Storage

If Aequora/application stores passwords:

```text
Argon2id
```

with modern parameters.

Better to integrate mature identity provider when possible.

---

# 231. Password Reset

Use:

```text
single-use token
short expiry
```

---

# 232. Email Verification

Token is single-use/bounded.

---

# 233. Invitation

Tenant invitation token must bind:

```text
tenant
role
expiry
```

---

# 234. Session Revocation

Security event may invalidate active sessions.

---

# 235. Device Registration

Require authenticated user/admin flow.

---

# 236. Device Key Rotation

Old key transitions to verification-only/revoked.

---

# 237. Lost Device

Revoke device and future access.

Already cached plaintext cannot be magically erased if device permanently offline.

---

# 238. Remote Purge

Best effort upon reconnect.

---

# 239. Security Event Model

Define:

```rust
pub struct SecurityEvent {
    pub event_id: SecurityEventId,
    pub kind: SecurityEventKind,
    pub tenant_id: Option<TenantId>,
    pub principal: Option<PrincipalId>,
    pub severity: SecuritySeverity,
}
```

---

# 240. Security Events

Examples:

```text
AuthFailureSpike
AuthorityRollbackDetected
ForkDetected
DeviceRevoked
AdminForcePromotion
KeyRevoked
CrossTenantAttempt
LegacyWriteAfterCutover
```

---

# 241. Security Audit vs Operational Alert

Audit:

```text
durable evidence
```

Alert:

```text
immediate response
```

Both may be generated.

---

# 242. Alert Routing

External SIEM/SOC integration future.

Core emits typed events.

---

# 243. SIEM Export

Could expose structured:

```text
JSON/CEF/syslog
```

adapter outside core.

---

# 244. Threat Intelligence

Not core responsibility.

---

# 245. Intrusion Detection

Infrastructure may provide.

Aequora supplies meaningful structured events.

---

# 246. Security Tests

Need dedicated suite.

---

# 247. Authentication Tests

Test:

```text
expired token
wrong tenant
revoked device
wrong audience
wrong issuer
```

---

# 248. Authorization Tests

Every operation kind:

```text
allowed role
denied role
cross-tenant
```

---

# 249. Replay Tests

Replay same signed operation.

Expected:

```text
one effect
```

---

# 250. Payload Substitution Test

Same OperationId new payload.

Expected:

```text
reject
```

---

# 251. Downgrade Test

Required capability stripped.

Expected:

```text
incompatible
```

---

# 252. Oversized Input Test

Boundary + 1 byte/element.

Expected:

```text
early reject
```

---

# 253. Compression Bomb Test

Expected decompressed bound.

---

# 254. Scope Injection Test

Untrusted filter attempts raw SQL semantics.

Expected:

```text
impossible via typed API
```

---

# 255. SSRF Test

URLs:

```text
localhost
127.0.0.1
::1
169.254.169.254
private RFC1918
redirect-to-private
DNS rebinding simulation
```

must be blocked per policy.

---

# 256. Archive Traversal Test

`../../etc/passwd`

rejected.

---

# 257. Cross-Tenant Blob Test

Tenant A tries object ref of B.

Denied.

---

# 258. Snapshot Swap Test

Manifest from Tenant B served to A.

Verification fails.

---

# 259. Authority Rollback Test

Client saw epoch 7, receives 6.

Fail closed.

---

# 260. Fork Test

Same epoch/checkpoint different digest.

Fail closed.

---

# 261. Admin Privilege Test

Read-only operator tries key destroy.

Denied.

---

# 262. Break-Glass Audit Test

Ensure critical audit event.

---

# 263. Payment Replay Test

Repeated provider request with same key.

One charge if provider supports idempotency.

---

# 264. Diagnostic Redaction Test

No secrets in incident bundle.

---

# 265. Fuzzing

Fuzz:

```text
protocol decoder
Postcard envelopes
snapshot parser
bundle parser
legacy import parser
admin JSON DTOs
```

---

# 266. Property Tests

Security properties:

```text
tenant isolation
idempotency
permission monotonicity
scope authorization
```

---

# 267. Model Checking

Part 01 can model:

```text
attacker replay
stale authority
duplicate operation
split brain
revocation timing
```

---

# 268. Penetration Testing

Before major production release:

```text
API
admin plane
auth
webhooks
uploads
```

should receive manual security testing.

---

# 269. Dependency Security CI

Recommended:

```text
cargo audit
cargo deny
cargo vet optional
license checks
```

---

# 270. Secret Scanning

CI scans repository for leaked secrets.

---

# 271. SAST

Clippy plus security-focused checks.

Rust memory safety already helps but does not solve logic bugs.

---

# 272. DAST

Run against staging API.

---

# 273. Security Review Gate

High-risk changes require review:

```text
auth/authz
crypto
payments
admin
authority
imports
webhooks
```

---

# 274. Threat Model Update

Part 27 is living documentation.

Any new subsystem adds:

```text
assets
entry points
abuse cases
```

---

# 275. Security ADR

Important security decisions get ADR.

Examples:

```text
why device signature optional
why one authority
why webhook SSRF block
```

---

# 276. Incident Response

When compromise suspected:

```text
revoke credentials
fence authority if needed
rotate keys
preserve audit evidence
generate forensic bundle
```

---

# 277. Key Compromise

Actions:

```text
mark revoked
activate new key
stop new use
retain verification metadata
```

---

# 278. Device Compromise

```text
revoke device
invalidate sessions
possibly purge on reconnect
```

---

# 279. Server Compromise

More severe.

Potential:

```text
rotate service credentials
rotate signing/encryption keys
validate audit checkpoints
rebuild node
```

---

# 280. DB Compromise

Need determine:

```text
read exposure
write tampering
journal/audit integrity
```

Signed checkpoints help detect history changes.

---

# 281. KMS Compromise

Rotate keys and assess confidentiality impact.

---

# 282. Provider Compromise

Suspend integration/circuit breaker, reconcile transactions.

---

# 283. Legacy System Compromise

Pause bridge, verify source integrity before resume.

---

# 284. Security Levels

Optional deployment profiles:

```text
Standard
Enterprise
HighAssurance
```

---

# 285. Standard

```text
TLS
strong auth
RBAC
rate limits
secret store
audit
```

---

# 286. Enterprise

Adds:

```text
mTLS internal
signed artifacts
centralized audit
SLO/security alerting
separation of duties
```

---

# 287. HighAssurance

Adds:

```text
external epoch anchor
hardware-backed keys
signed audit checkpoints
two-person destructive approval
restricted admin network
```

---

# 288. Do Not Fake Certification

Profiles are architecture configurations, not legal/compliance certifications.

---

# 289. Security Configuration

Example RON:

```ron
security: (
    protocol: (
        max_frame_bytes: 4194304,
        max_operations_per_batch: 1000,
        max_dependency_edges: 5000,
    ),

    auth: (
        require_device_binding: true,
    ),

    admin: (
        private_listener: true,
        require_mfa_for_destructive: true,
    ),

    webhooks: (
        block_private_networks: true,
        max_redirects: 0,
    ),

    uploads: (
        max_blob_bytes: 104857600,
    ),
)
```

---

# 290. Safe Defaults

Ship with:

```text
private admin
bounded requests
no raw SQL filters
no public object storage
no insecure crypto downgrade
```

---

# 291. Security Error Codes

Examples:

```text
AUTH_INVALID
AUTH_REVOKED
AUTHZ_DENIED
TENANT_MISMATCH
PROTOCOL_DOWNGRADE
PAYLOAD_MISMATCH
SNAPSHOT_SIGNATURE_INVALID
AUTHORITY_ROLLBACK
SSRF_BLOCKED
```

---

# 292. Client Error Detail

Expose enough for recovery.

Do not expose internal secret information.

---

# 293. Audit Security Decision

Important denied operations may be audited:

```text
admin denied
cross-tenant attempt
repeated signature failure
```

Avoid logging every random Internet scan into expensive business audit if not useful.

---

# 294. Security Telemetry

Metrics:

```text
auth_failure_total
authz_denied_total
device_revoked_attempt_total
protocol_downgrade_rejected_total
ssrf_blocked_total
signature_invalid_total
cross_tenant_denied_total
```

---

# 295. Cardinality

No principal/device IDs as metrics labels.

Use traces/logs.

---

# 296. Alert Thresholds

Example:

```text
sudden auth failure spike
many cross-tenant attempts
authority rollback
fork
admin force operation
```

---

# 297. Security Runbooks

Document:

```text
stolen device
compromised key
suspected DB tamper
provider incident
admin credential compromise
split brain
```

---

# 298. Security Invariants

Add:

## AEQ-INV-SEC001

```text
No client-supplied tenant, role, scope, priority, or authority claim is accepted as authorization evidence without server validation.
```

## AEQ-INV-SEC002

```text
The same OperationId cannot produce different authoritative semantics by changing its payload.
```

## AEQ-INV-SEC003

```text
A required security capability is never silently downgraded during compatibility negotiation.
```

## AEQ-INV-SEC004

```text
Every externally controlled collection, payload, archive, graph, and upload has an explicit size/complexity bound.
```

## AEQ-INV-SEC005

```text
Cross-tenant data access is denied even when the attacker knows valid entity, scope, blob, operation, or snapshot identifiers.
```

## AEQ-INV-SEC006

```text
Private cryptographic keys and authentication secrets never appear in logs, ordinary diagnostics, audit payloads, or API responses.
```

---

# 299. Additional Invariants

## AEQ-INV-SEC007

```text
A stale or rolled-back authority epoch cannot silently resume trusted synchronization.
```

## AEQ-INV-SEC008

```text
External side effects that can cause financial or irreversible outcomes use explicit idempotency/reconciliation semantics.
```

## AEQ-INV-SEC009

```text
Administrative override paths are more strongly authorized and audited than ordinary operational paths.
```

## AEQ-INV-SEC010

```text
Legacy, import, webhook, diagnostic, and provider inputs are treated as untrusted regardless of source network location.
```

---

# 300. Threat Matrix

Recommended maintained document/table:

```text
Threat
Asset
Entry Point
Precondition
Mitigation
Detection
Test
Residual Risk
```

---

# 301. Example Threat Matrix Entry

```text
Threat:
    Cross-tenant EntityId probing

Asset:
    Tenant business data

Entry:
    Sync operation / read API

Mitigation:
    AuthContext tenant binding
    object-level authorization
    tenant-scoped repository query

Detection:
    authz denied security event

Test:
    cross-tenant property test

Residual:
    timing side channel minimized but not necessarily zero
```

---

# 302. Security Ownership

Every crate/subsystem should name:

```text
security-sensitive APIs
owner/reviewer
```

---

# 303. Security-Critical Crates

Likely:

```text
aequora-auth
aequora-protocol
aequora-crypto
aequora-authority
aequora-admin
aequora-side-effects
aequora-governance
```

---

# 304. Unsafe Boundary

If unsafe code ever enters security-critical crate:

```text
separate review
fuzz
Miri where applicable
```

---

# 305. Miri

Useful for unsafe/internal UB checks in applicable tests.

---

# 306. Loom

Use for in-process concurrent security-sensitive state.

---

# 307. Fuzz Corpus

Retain real bug-derived sanitized inputs.

---

# 308. Security Regression Corpus

Any discovered vulnerability gets regression test before close.

---

# 309. Release Security Checklist

Before production release:

```text
dependency advisories reviewed
threat model updated
auth/authz tests pass
fuzzers healthy
secret scan clean
admin permissions reviewed
protocol limits verified
```

---

# 310. Completion Criteria

Part 27 is complete when:

```text
[ ] assets enumerated
[ ] attacker classes defined
[ ] trust boundaries defined
[ ] client/server trust rules defined
[ ] auth/authz threat model defined
[ ] replay/substitution/downgrade defenses defined
[ ] tenant isolation defenses defined
[ ] SSRF/webhook/payment threats defined
[ ] upload/import/archive threats defined
[ ] local-device threat model defined
[ ] authority/replica threat model defined
[ ] admin/insider controls defined
[ ] supply-chain controls defined
[ ] secrets/key handling defined
[ ] abuse/resource-exhaustion defenses defined
[ ] security test matrix defined
[ ] incident response paths defined
[ ] security invariants added
```

---

# 311. Final Architecture

```text
                   UNTRUSTED CLIENT / INTERNET
                              │
                              ▼
                       TLS / Edge Limits
                              │
                              ▼
                       Authentication
                              │
                              ▼
                     Tenant/Device Binding
                              │
                              ▼
                       Authorization
                              │
                              ▼
                 Bounded Protocol Validation
                              │
                              ▼
                    Typed Domain Operation
                              │
                              ▼
                    Authoritative Execution
                              │
                 ┌────────────┼────────────┐
                 ▼            ▼            ▼
              Journal       Audit       SideEffect
                 │            │            │
                 ▼            ▼            ▼
            Integrity      Evidence      Provider
                 │                         │
                 └────────────┬────────────┘
                              ▼
                     Security Monitoring

Privileged path:

Administrator
    │
    ▼
Strong Auth / MFA / Private Network
    │
    ▼
Explicit Permission / Approval
    │
    ▼
Typed Admin Operation
    │
    ▼
Audit + Verification
```

The architectural principle is:

> **Aequora should remain secure even when clients lie, networks are hostile, devices are compromised, providers fail ambiguously, legacy systems behave unexpectedly, and operators make mistakes.**

By combining strong server-side authorization, bounded protocol parsing, tenant isolation, idempotency, cryptographic integrity, authority rollback protection, hardened integration boundaries, least-privilege operations, supply-chain controls, and dedicated abuse testing, Aequora can treat security as an architectural property rather than a final release checklist.
