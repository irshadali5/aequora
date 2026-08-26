# Aequora Sync — Part 25

# Diagnostics, Forensics, and Reproducible Incident Bundle Architecture

## 1. Purpose

A production synchronization system eventually encounters incidents that are difficult to understand from a single error message.

Examples:

```text
client says "sync failed"
server says operation committed
device retries same OperationId
scope cursor seems behind
snapshot verifies but client state differs
authority epoch changed during reconnect
job worker retried provider call
local DB crashed during reconciliation
anti-entropy found a divergent partition
```

Debugging these cases requires evidence from several layers:

```text
client
server
database adapter
protocol
journal
operation ledger
audit
scope state
authority state
background jobs
integrity state
runtime configuration
software versions
```

The challenge is to collect enough evidence to explain and reproduce the problem without:

```text
dumping the full customer database
exposing secrets
leaking private keys
collecting unnecessary PII
creating an unbounded diagnostic archive
```

The central rule is:

> **Aequora diagnostics should capture the smallest sufficient, typed, verifiable evidence set needed to explain an incident and reproduce the relevant state transition.**

---

# 2. Goals

The diagnostics architecture should provide:

```text
structured runtime diagnostics
operation-level explainability
client/server correlation
incident bundle generation
reproducible failure traces
sanitized metadata export
adapter/runtime inventory
crash-state capture
integrity evidence
authority timeline evidence
job/workflow evidence
privacy controls
support tooling
```

---

# 3. Non-Goals

The diagnostics subsystem is not:

```text
a full database backup
a general data warehouse
a perpetual packet capture
a secret dump
a replacement for observability metrics/logging
```

---

# 4. Four Diagnostic Layers

Separate:

```text
1. Runtime Observability
2. Structured Diagnostic State
3. Incident Bundle
4. Reproduction Harness
```

---

# 5. Runtime Observability

Includes:

```text
metrics
tracing
structured logs
health
queue depth
latency
```

Useful for:

```text
detecting something is wrong
```

---

# 6. Structured Diagnostic State

Small durable metadata retained specifically for support.

Examples:

```text
last sync attempts
last cursor transition
last authority epoch seen
last conflict
last bootstrap state
last scheduler decision
```

---

# 7. Incident Bundle

A bounded artifact assembled for one incident.

Contains:

```text
manifest
sanitized metadata
relevant logs/traces
operation lineage
version inventory
integrity evidence
optional replay inputs
```

---

# 8. Reproduction Harness

Uses captured bundle to recreate:

```text
protocol input
state preconditions
handler version
execution inputs
adapter behavior
```

where possible.

---

# 9. IncidentId

Define:

```rust
pub struct IncidentId(Uuid);
```

Use UUIDv7.

---

# 10. IncidentBundleId

```rust
pub struct IncidentBundleId(Uuid);
```

One incident may have multiple bundles:

```text
client bundle
server bundle
combined bundle
```

---

# 11. Incident Class

```rust
pub enum IncidentClass {
    SyncFailure,
    Divergence,
    ConflictAnomaly,
    BootstrapFailure,
    AuthorityTransition,
    JobFailure,
    DataIntegrity,
    Performance,
    Security,
    Governance,
    Unknown,
}
```

---

# 12. Diagnostic Scope

Bundle should be explicitly scoped.

Examples:

```text
OperationId
EntityRef
ScopeId
DeviceId
JobId
Incident time window
```

---

# 13. Scope Before Collection

Never start with:

```text
export everything
```

Start with the strongest known correlation key.

---

# 14. DiagnosticSelector

Conceptually:

```rust
pub enum DiagnosticSelector {
    Operation(OperationId),
    Entity(EntityRef),
    Scope(ScopeId),
    Device(DeviceId),
    Job(JobId),
    TimeWindow(TimeRange),
}
```

---

# 15. Correlation Keys

Most useful identifiers:

```text
OperationId
CorrelationId
CausationId
EventId
AuditEventId
JobId
WorkflowId
ScopeId
AuthorityEpoch
```

---

# 16. Operation-Centric Forensics

Given `OperationId`, diagnostics should reconstruct:

```text
client outbox state
first send attempt
server ledger result
journal event(s)
audit event(s)
conflict record
job/side-effect intents
client reconciliation result
```

---

# 17. Operation Explain Graph

Conceptually:

```text
OperationId
   │
   ├── OutboxRecord
   ├── OperationLedger
   ├── JournalEvent(s)
   ├── AuditEvent(s)
   ├── SideEffectIntent(s)
   └── ClientReconcileRecord
```

---

# 18. Entity-Centric Forensics

Given `EntityRef`, show:

```text
current authoritative version
current client version
latest journal events
latest audit changes
scope memberships
tombstone state
field provenance
```

---

# 19. Scope-Centric Forensics

Given `ScopeId`, show:

```text
scope version
scope generation
projection schema
client cursor
server journal floor
snapshot boundary
membership strategy
last bootstrap
```

---

# 20. Device-Centric Forensics

Show:

```text
DeviceId
status
client build
last seen
last known authority epoch
scope watermarks
local store generation
```

Avoid exposing unnecessary user PII.

---

# 21. Authority-Centric Forensics

Show:

```text
AuthorityId
AuthorityEpoch
current sequence
promotion history
journal checkpoints
fork evidence
```

---

# 22. Job-Centric Forensics

Given `JobId`, show:

```text
job payload schema
state
attempts
worker leases
fencing tokens
provider outcomes
workflow relation
side-effect intent
```

---

# 23. Local Diagnostic Ring

Clients maintain small bounded ring buffer.

Suggested events:

```text
SyncStarted
SyncBatchSent
SyncResponseReceived
CursorAdvanced
RetryScheduled
AuthorityChanged
BootstrapStarted
BootstrapCheckpoint
ConflictRecorded
ResourceDeferred
```

---

# 24. Diagnostic Ring Size

Bound by:

```text
event count
bytes
time
```

Example:

```text
last 200–1000 events
```

depending profile.

---

# 25. DiagnosticEvent

```rust
pub struct DiagnosticEvent {
    pub event_id: DiagnosticEventId,
    pub kind: DiagnosticEventKind,
    pub occurred_at: Timestamp,
    pub correlation_id: Option<CorrelationId>,
    pub operation_id: Option<OperationId>,
    pub scope_id: Option<ScopeId>,
    pub details: DiagnosticDetails,
}
```

---

# 26. Structured Details

Use typed bounded fields.

Avoid:

```text
arbitrary giant debug strings
```

---

# 27. Redaction by Construction

Sensitive fields should be represented as:

```text
redacted
digest
length
classification
```

rather than raw value.

---

# 28. DiagnosticValue

Conceptually:

```rust
pub enum DiagnosticValue {
    Text(String),
    Number(i64),
    Id(Uuid),
    Digest([u8; 32]),
    Redacted,
}
```

---

# 29. PII Classification

Use Part 14 governance classification.

Diagnostic exporter consults:

```text
DataClass
AuditValuePolicy
```

---

# 30. Secret Classification

Never include:

```text
passwords
access tokens
refresh tokens
private keys
DEKs
API secrets
```

---

# 31. Secret Redaction

Redaction happens before bundle creation.

Not after upload.

---

# 32. Bundle Manifest

Every incident bundle begins with human-readable RON manifest.

Example:

```ron
(
    bundle_id: "...",
    incident_id: "...",
    created_at: "...",
    producer: Client,
    incident_class: SyncFailure,
    selectors: [...],
    schema_version: 1,
)
```

---

# 33. Bundle Format

Recommended:

```text
incident-bundle/
├── manifest.ron
├── inventory.ron
├── timeline.postcard
├── metadata/
├── traces/
├── logs/
├── replay/
├── integrity/
└── hashes.ron
```

Then optionally:

```text
tar + zstd
```

---

# 34. BundleSchemaVersion

```rust
pub struct IncidentBundleSchemaVersion(u16);
```

---

# 35. Inventory

Record:

```text
Aequora crate version
application build
OS
architecture
database adapter
database version
protocol version
schema versions
capabilities
authority epoch
```

---

# 36. Client Inventory

Example:

```text
platform
app build
Aequora build
local adapter
LocalStoreFormatVersion
MetadataSchemaVersion
```

---

# 37. Server Inventory

Example:

```text
server build
Postgres adapter
protocol policy
handler registry version
crypto policy generation
compatibility generation
```

---

# 38. Environment Information

Include only relevant safe metadata.

Good:

```text
OS version
CPU architecture
memory class
```

Avoid:

```text
full environment variable dump
```

---

# 39. Configuration Snapshot

Capture effective non-secret configuration relevant to incident.

Examples:

```text
sync batch limits
scheduler policy
compatibility policy
scope descriptor
```

---

# 40. Config Redaction

Secret references:

```text
KMS key ID
secret name
```

okay.

Secret value:

```text
never
```

---

# 41. Timeline

Bundle timeline should order:

```text
client diagnostic events
server trace events
journal sequences
job attempts
authority transitions
```

by:

```text
authoritative sequence
event timestamp
correlation
```

---

# 42. Timestamp Caveat

Client wall clock may be wrong.

Mark each time with source:

```text
ClientWallClock
ServerAuthorityTime
MonotonicRelative
```

---

# 43. TimelineEvent

```rust
pub struct TimelineEvent {
    pub source: TimelineSource,
    pub logical_order: Option<u64>,
    pub timestamp: Timestamp,
    pub kind: TimelineKind,
    pub references: TimelineRefs,
}
```

---

# 44. Logical Order

Use:

```text
journal sequence
audit sequence
local operation sequence
job attempt
```

when available.

---

# 45. Trace Capture

Use OpenTelemetry/tracing IDs.

Bundle relevant spans around:

```text
operation
sync exchange
job
bootstrap
```

---

# 46. Trace Time Window

Default:

```text
incident ± small bounded window
```

not hours of unrelated traffic.

---

# 47. Log Capture

Structured logs only.

Filter by:

```text
OperationId
CorrelationId
JobId
ScopeId
```

---

# 48. Log Sanitization

Redact at logging source.

Bundle exporter applies second policy gate.

---

# 49. Operation Ledger Evidence

Include bounded record:

```text
status
payload digest
committed sequence
handler version
result code
```

---

# 50. Journal Evidence

Include only relevant journal events.

For entity incident:

```text
last N before
incident event
next N after
```

---

# 51. Audit Evidence

Include:

```text
AuditEventId
action
actor class
reason code
changes according to diagnostic policy
```

---

# 52. Field Provenance

Useful for:

```text
why does this field have this value?
```

Bundle can include field provenance pointer chain.

---

# 53. Scope Evidence

Include:

```text
ScopeId
ScopeVersion
ScopeGeneration
ProjectionSchemaVersion
resolved authorized parameters
```

---

# 54. Do Not Include Raw Authorization Secrets

Include:

```text
role IDs
policy version
decision code
```

---

# 55. Snapshot Evidence

For bootstrap incidents:

```text
SnapshotId
manifest digest
boundary
chunk IDs
verified hashes
install states
```

---

# 56. Snapshot Chunk Content

Do not include chunk payload by default.

Include:

```text
digest
size
status
```

---

# 57. Integrity Evidence

For divergence:

```text
expected root
actual root
partition path
Merkle proof/path
```

---

# 58. Anti-Entropy Proof

This can often localize mismatch without exposing unrelated records.

---

# 59. Fork Evidence

Part 16:

```text
same epoch
same checkpoint sequence
different journal roots
```

Bundle should preserve signed checkpoints.

---

# 60. Crypto Evidence

Include:

```text
key IDs
algorithm IDs
signature verification result
registry generation
```

Never key material.

---

# 61. Compatibility Evidence

Include:

```text
ClientHello summary
ServerHello summary
selected protocol
required capabilities
compatibility result
```

---

# 62. Resource Evidence

For performance/low-resource incidents:

```text
memory profile
storage state
network type
battery policy state
```

coarse only.

---

# 63. Admission Evidence

For overload:

```text
LoadState
queue depth
rejection reason
tenant/class budget state
```

---

# 64. Job Attempt Evidence

For side-effect incidents:

```text
attempt number
worker ID pseudonymous/operational
fence token
provider result class
retry decision
```

---

# 65. Provider Response

Do not include full provider response by default.

Use:

```text
HTTP status
provider error code
response digest
sanitized fields
```

---

# 66. Replay Input

Part 12 can include:

```text
canonical operation
ExecutionInputs
handler version
policy digest
pre-state digest/reference
expected plan digest
```

---

# 67. Replay Bundle vs Incident Bundle

Replay bundle is narrower and execution-focused.

Incident bundle may embed or reference one replay bundle.

---

# 68. Reproduction Level

Define:

```rust
pub enum ReproductionLevel {
    MetadataOnly,
    ProtocolReplay,
    DomainReplay,
    FullLocalSimulation,
}
```

---

# 69. MetadataOnly

Enough to explain state transitions.

---

# 70. ProtocolReplay

Replays:

```text
request decode
negotiation
validation
```

without production DB.

---

# 71. DomainReplay

Runs deterministic domain handler with captured execution inputs.

---

# 72. FullLocalSimulation

Uses:

```text
in-memory/reference store
client state
server model
network fault script
```

for complex synchronization bug.

---

# 73. Fault Script

Part 01 deterministic simulator can consume:

```text
drop response
duplicate request
crash after commit
reorder message
```

---

# 74. Incident Reproduction Manifest

Example:

```ron
(
    seed: 8821,
    actions: [
        LocalMutate(...),
        Send(...),
        CommitServer(...),
        DropResponse,
        RestartClient,
        Retry(...),
    ],
)
```

---

# 75. Failure Trace Minimization

Use property-test shrinking/model checker shrinking to reduce failing sequence.

---

# 76. Minimal Reproducer

Goal:

```text
fewest operations
fewest entities
fewest faults
```

that still reproduces.

---

# 77. Canonical Bundle Hash

Compute BLAKE3 over bundle manifest/content inventory.

---

# 78. Signed Bundle

High-assurance mode may sign incident bundle manifest.

Useful for:

```text
forensic custody
compliance evidence
```

---

# 79. Chain of Custody

Optional fields:

```text
created_by
exported_by
transferred_at
verified_by
```

---

# 80. Forensic Mode

High-assurance incident:

```rust
DiagnosticMode::Forensic
```

may retain stronger evidence and signatures.

---

# 81. Support Mode

Default:

```rust
DiagnosticMode::Support
```

more privacy-minimized.

---

# 82. Developer Mode

May include richer local debug metadata in non-production.

---

# 83. Production Defaults

Production should default:

```text
sanitized
bounded
minimal
```

---

# 84. Consent/Authorization

Creating bundle containing tenant/customer data requires authorization.

---

# 85. Tenant Scope

Support operator may generate bundle only for authorized tenant.

---

# 86. Bundle Encryption

Incident bundles often contain sensitive metadata.

Encrypt before storage/transfer.

Part 15 applies.

---

# 87. Bundle Recipient

Options:

```text
organization support public key
tenant public key
one-time passphrase
KMS
```

---

# 88. Bundle Expiry

Default short retention.

---

# 89. Bundle Registry

Logical:

```text
aequora_incident_bundle
```

Fields:

```text
bundle_id
incident_id
tenant_id
mode
state
artifact_ref
digest
created_at
expires_at
created_by
```

---

# 90. Bundle State

```text
Planning
Collecting
Sanitizing
Verifying
Ready
Failed
Expired
```

---

# 91. Durable Bundle Job

Large bundle generation uses Part 23 job.

---

# 92. Bundle Plan

First determine:

```text
selectors
sources
time window
classification
redaction policy
```

---

# 93. Bundle Plan Preview

Admin can see:

```text
which record types
estimated size
sensitivity classes
```

before execution.

---

# 94. Plan Digest

Part 24 plan/execute model applies.

---

# 95. Client-Initiated Support Bundle

Client can generate local support bundle.

Must be:

```text
small
redacted
```

---

# 96. Client Bundle Contents

Recommended:

```text
manifest
app/runtime inventory
last diagnostic events
scope cursors
outbox metadata
last errors
resource context
```

---

# 97. Client Outbox Payload

Do not include raw payload by default.

Include:

```text
OperationId
kind
schema
digest
state
```

---

# 98. Optional Payload Inclusion

Only for explicitly selected operation and policy-approved fields.

---

# 99. Server Bundle

May include:

```text
ledger
journal
audit
trace
job
scope
authority
```

for same correlation set.

---

# 100. Combined Bundle

Server-side tool can merge:

```text
client bundle
server bundle
```

using:

```text
OperationId
CorrelationId
AuthorityEpoch
```

---

# 101. Clock Alignment

Client/server wall clocks may differ.

Use known sync exchange timing to estimate offset for display.

Do not rewrite authoritative timestamps.

---

# 102. Network Round Trip Evidence

Capture:

```text
request start
response received
request ID
HTTP status
```

---

# 103. Transport Payload Capture

Do not packet-capture full TLS payload by default.

Protocol metadata is safer and more useful.

---

# 104. Raw Wire Capture

Developer/test mode only, with explicit sensitivity warning.

---

# 105. Database Query Evidence

For performance incident:

```text
query ID
duration
rows
plan hash
```

rather than full SQL values.

---

# 106. SQL Redaction

Parameterized SQL template okay.

Bind values may be sensitive.

---

# 107. Query Plan

Postgres:

```text
EXPLAIN plan
```

can be included if relevant.

---

# 108. Adapter Evidence

Every adapter should expose:

```rust
pub trait DiagnosticProvider {
    async fn diagnostics(
        &self,
        request: DiagnosticRequest,
    ) -> Result<DiagnosticSection, DiagnosticError>;
}
```

---

# 109. Adapter Diagnostic Contract

Must provide:

```text
version
capabilities
store health
schema version
selected counters
```

without exposing secret config.

---

# 110. PostgreSQL Adapter Diagnostics

Possible:

```text
pool usage
server version
metadata schema
transaction isolation
replication state
```

---

# 111. Stoolap Adapter Diagnostics

Possible:

```text
store size
schema version
transaction mode
index health
```

---

# 112. Cross-DB Diagnostics

Bundle uses canonical fields.

Avoid DB-specific assumptions in core incident analysis.

---

# 113. Diagnostic Section IDs

Stable IDs for:

```text
Runtime
Outbox
Journal
Ledger
Scope
Authority
Job
Audit
Integrity
Crypto
Compatibility
```

---

# 114. Section Versioning

Each section has:

```text
schema version
```

independent from whole bundle where useful.

---

# 115. Missing Section

Bundle should explicitly state:

```text
Unavailable
NotAuthorized
NotApplicable
CollectionFailed
```

not silently omit.

---

# 116. Collection Failure

A partial bundle can still be useful.

State:

```text
ReadyWithWarnings
```

may be allowed.

---

# 117. Required Section

For forensic mode, some sections may be mandatory.

If missing:

```text
bundle not verified complete
```

---

# 118. Bundle Completeness

Manifest lists:

```text
requested sections
collected sections
missing sections
```

---

# 119. Hash Inventory

`hashes.ron`:

```text
relative path
BLAKE3 digest
size
```

---

# 120. Verification Command

```text
aequora incident verify bundle.aeqincident
```

checks:

```text
hashes
signature
schema
```

---

# 121. CLI

Suggested:

```text
aequora incident create
aequora incident verify
aequora incident inspect
aequora incident explain-operation
aequora incident replay
aequora incident minimize
```

---

# 122. Explain Operation CLI

Example:

```text
aequora incident explain-operation <OperationId>
```

Output:

```text
client state
server status
journal seq
audit action
job result
```

---

# 123. Explain Entity CLI

```text
aequora incident explain-entity <EntityRef>
```

---

# 124. Explain Cursor

```text
aequora incident explain-cursor <ScopeId>
```

shows:

```text
client cursor
scope generation
journal floor
snapshot boundary
```

---

# 125. Replay CLI

```text
aequora incident replay bundle.aeqincident
```

---

# 126. Replay Isolation

Never replay against production side effects.

Use:

```text
in-memory store
sandbox DB
mock providers
```

---

# 127. Side Effect Sink

Replay uses:

```text
NoopSideEffectSink
CapturedSideEffectSink
```

---

# 128. Reproduction Determinism

Replay should verify:

```text
plan digest
outcome digest
```

---

# 129. Handler Mismatch

If historical handler version unavailable:

```text
report
```

do not pretend exact replay.

---

# 130. Compatibility Upcaster

Bundle may need old operation payload upcaster.

---

# 131. Build Artifact Retention

For serious production support, retain:

```text
symbols
source commit
handler build metadata
```

for supported releases.

---

# 132. BuildIdentity

```rust
pub struct BuildIdentity {
    pub version: String,
    pub git_commit: Option<String>,
    pub build_id: BuildId,
}
```

---

# 133. Git Commit Privacy

For open-source/self-hosted, okay.

For private app, build ID may be enough.

---

# 134. Source Reproducibility

CI can retain SBOM/lockfile/build provenance.

---

# 135. Dependency Inventory

Incident bundle may include:

```text
crate versions
feature flags
```

as SBOM digest/reference, not giant list by default.

---

# 136. Cargo.lock Digest

Useful:

```text
Cargo.lock BLAKE3 digest
```

---

# 137. Feature Flag Inventory

Important because behavior can differ.

Include enabled Aequora features.

---

# 138. Runtime Feature Gates

Include:

```text
compatibility features
brownout state
crypto requirements
```

---

# 139. Dynamic Config Generation

Include:

```text
ConfigGeneration
```

and relevant sanitized values.

---

# 140. Incident Snapshot of Policies

For replay/explanation, record:

```text
consistency profile version
handler version
governance policy version
crypto policy version
compatibility policy generation
```

---

# 141. Policy Drift

Current configuration may differ from incident-time configuration.

Bundle should preserve incident-time identifiers/digests.

---

# 142. Diagnostic Retention

Diagnostic data itself needs Part 14 retention policy.

---

# 143. Default Retention

Short.

Example product policy may choose:

```text
7–30 days
```

but core does not hardcode.

---

# 144. Legal Hold

Incident bundle may itself be placed under legal hold.

---

# 145. Erasure

If bundle contains erased subject data:

```text
governance plan includes bundle
```

unless legal retention applies.

---

# 146. Audit

Creating/exporting/deleting forensic bundle is audited.

---

# 147. Access Audit

Viewing highly sensitive forensic bundle can be audited.

---

# 148. Bundle Storage

Use separate protected object storage namespace.

---

# 149. Bundle URL

Short-lived signed URL only.

---

# 150. No Public Link

Never create anonymous long-lived diagnostic link.

---

# 151. Download Protection

Part 15 encryption + auth.

---

# 152. Support Upload

Client may upload bundle to server.

Validate:

```text
size
schema
hashes
content type
```

---

# 153. Untrusted Bundle

Never deserialize arbitrary uploaded bundle without bounds.

---

# 154. Path Traversal

Archive extraction must reject:

```text
../
absolute paths
symlink escapes
```

---

# 155. Zip/Tar Bomb

Enforce:

```text
compressed size
uncompressed size
file count
nesting
```

---

# 156. Parser Fuzzing

Fuzz bundle parsers.

---

# 157. Signature Does Not Make Content Safe

Even signed bundle parser must enforce bounds.

---

# 158. Incident Bundle Size

Use profile limits.

Examples:

```text
client small: < 10 MB
server standard: < 100 MB
forensic large: explicit approval
```

Actual numbers configurable.

---

# 159. Bundle Size Estimation

Plan before collecting.

---

# 160. Truncation

If section exceeds budget:

```text
truncate oldest/least relevant
```

and record truncation.

---

# 161. Sampling

For repetitive logs:

```text
sample
```

but preserve first/last/errors.

---

# 162. Timeline Window

Default bounded.

For known OperationId, may not need broad time window.

---

# 163. Diagnostic Index

Server can index:

```text
OperationId -> trace ID
JobId -> attempts
CorrelationId -> logs
```

for fast bundle collection.

---

# 164. No Full Log Scan

Structured log backend should support indexed fields.

---

# 165. Local Log Index

Client diagnostic ring already small.

---

# 166. Error Fingerprint

Define stable diagnostic fingerprint from:

```text
error code
phase
handler version
top stack frame/build ID
```

---

# 167. Fingerprint Use

Group recurring incidents without uploading full details.

---

# 168. Crash Report

Crash artifact:

```text
build ID
panic message redacted
backtrace/symbol refs
last diagnostic events
```

---

# 169. Panic Hook

Client/server can capture bounded panic metadata.

---

# 170. No Heap Dump by Default

Heap dumps may contain secrets/PII.

Require explicit forensic mode.

---

# 171. Core Dump

Same.

Not part of ordinary incident bundle.

---

# 172. Backtrace

Useful and usually safer than heap.

Still inspect for path/user data leakage.

---

# 173. Symbolization

Keep separate debug symbols.

Incident bundle can include:

```text
build ID
instruction addresses
```

Support tool symbolizes later.

---

# 174. Source Paths

May reveal developer usernames.

Normalize build paths where possible.

---

# 175. Panic Payload Redaction

Avoid panics containing raw domain values.

---

# 176. Typed Errors

Aequora should prefer stable error codes.

---

# 177. ErrorCode Registry

Stable numeric IDs.

Part 29 can formalize registry governance.

---

# 178. Error Context

Attach:

```text
phase
operation kind
adapter
```

not raw payload.

---

# 179. Phase IDs

Examples:

```text
Decode
Authenticate
Authorize
Validate
Plan
Commit
Reconcile
Bootstrap
```

---

# 180. Forensic Query API

Admin plane can expose:

```text
GET /forensics/operation/{id}
GET /forensics/job/{id}
GET /forensics/scope/{id}
```

---

# 181. Read Authorization

Support user gets only authorized tenant.

Security/admin forensic role required for broader access.

---

# 182. Forensic Read Source

Critical current state should read authority.

Historical logs may come from observability store.

---

# 183. No Side Effects

Forensic reads are read-only.

---

# 184. Incident Bundle Generation Flow

```text
request
↓
authorize
↓
plan selectors/sections
↓
estimate size/sensitivity
↓
approve if required
↓
collect
↓
sanitize
↓
hash
↓
encrypt/sign
↓
verify
↓
publish
```

---

# 185. Collection Isolation

Bundle generation should not heavily impact production.

Use Part 18:

```text
Background/Maintenance
```

unless critical incident.

---

# 186. Consistent View

For authoritative metadata, bundle may capture boundary:

```text
journal sequence N
```

so sections can be interpreted consistently.

---

# 187. Snapshot Transaction

Small metadata bundle may use one read-only repeatable transaction.

Large bundle uses explicit sequence boundary.

---

# 188. BundleBoundary

```rust
pub struct BundleBoundary {
    pub authority_epoch: AuthorityEpoch,
    pub journal_sequence: Sequence,
}
```

---

# 189. Historical Data Beyond Boundary

Mark separately.

---

# 190. Client Bundle Boundary

Use:

```text
local cursor
local operation sequence
```

---

# 191. Merge Client/Server Bundle

Record both boundaries.

---

# 192. Evidence Confidence

Each section may indicate:

```rust
pub enum EvidenceConfidence {
    Authoritative,
    DurableLocal,
    Derived,
    BestEffort,
}
```

---

# 193. Authoritative

Examples:

```text
operation ledger
journal
audit
```

---

# 194. DurableLocal

Client outbox/cursor.

---

# 195. Derived

Cache/projection.

---

# 196. BestEffort

Logs/presence/live state.

---

# 197. Explainability Output

Support tool should say:

```text
fact
source
confidence
```

---

# 198. Example Operation Explanation

```text
Operation 01J...
Client:
  state: Retryable
  ever_sent: true

Authority:
  ledger: Accepted
  committed sequence: 8821

Client cursor:
  8819

Conclusion:
  server committed operation, but client has not reconciled through sequence 8821.
```

This is far more useful than raw logs.

---

# 199. Root Cause Classification

Tool may classify:

```text
TransportLossAfterCommit
ClientReconcileFailure
CursorExpired
ProtocolIncompatible
AuthorityChanged
ProviderAmbiguity
```

---

# 200. Inference Label

If conclusion is inferred rather than proven:

```text
Likely
```

not `Confirmed`.

---

# 201. No AI Requirement

Diagnostic architecture should work deterministically without AI.

AI can assist interpretation later, but evidence model must stand alone.

---

# 202. AI Privacy

If future AI support analyzes bundle:

```text
explicit policy
redaction
tenant consent/config
```

---

# 203. Machine-Readable Incident Summary

```rust
pub struct IncidentSummary {
    pub classification: IncidentClassification,
    pub confidence: EvidenceConfidence,
    pub supporting_refs: Vec<EvidenceRef>,
}
```

---

# 204. Reproducer Registry

Keep known incident reproducers in test corpus.

---

# 205. Regression Test

Once bug fixed:

```text
incident bundle/reduced trace
→ CI regression case
```

---

# 206. Privacy-Preserving Fixture

Replace real PII with synthetic equivalents.

---

# 207. Bundle Sanitizer

```rust
pub trait DiagnosticSanitizer {
    fn sanitize(
        &self,
        section: DiagnosticSection,
        policy: &DiagnosticPolicy,
    ) -> SanitizedSection;
}
```

---

# 208. DiagnosticPolicy

```rust
pub struct DiagnosticPolicy {
    pub mode: DiagnosticMode,
    pub max_bytes: u64,
    pub include_payloads: bool,
    pub include_audit_values: bool,
    pub include_logs: bool,
}
```

---

# 209. Policy Presets

```text
SupportMinimal
SupportStandard
Forensic
Developer
```

---

# 210. SupportMinimal

Only:

```text
IDs
digests
state
versions
errors
```

---

# 211. SupportStandard

Adds bounded:

```text
sanitized traces
selected metadata
```

---

# 212. Forensic

Adds:

```text
signed evidence
broader timeline
policy-approved sensitive fields
```

---

# 213. Developer

May include raw test data in nonproduction.

---

# 214. Production Guard

`Developer` mode disabled in production by default.

---

# 215. Bundle Encryption Requirement

Production bundle with tenant data should normally be encrypted.

---

# 216. Bundle Signature

Optional standard, required in forensic profile.

---

# 217. Bundle Verification

Before support uses bundle:

```text
hash verify
signature verify
schema validate
```

---

# 218. Bundle Compatibility

Part 21: older support tooling may read older bundle schema.

Use upcasters for metadata-only changes.

---

# 219. Reproduction Compatibility

Exact domain replay requires historical handler support.

---

# 220. Diagnostic Store

Server may maintain temporary incident records.

Do not create a permanent shadow data lake.

---

# 221. Incident State

```text
Open
Investigating
Mitigated
Resolved
Closed
```

Could live in external incident system; Aequora does not need full incident management.

---

# 222. External Incident Integration

Expose:

```text
IncidentId
bundle artifact
summary
```

for Jira/PagerDuty/etc. through application integration.

---

# 223. No Vendor Dependency

Core stays neutral.

---

# 224. Observability Correlation

Tracing/logging should include:

```text
OperationId
CorrelationId
JobId
```

when applicable.

---

# 225. Avoid High-Cardinality Metrics

These IDs belong in traces/logs, not Prometheus labels.

---

# 226. Metrics

```text
incident_bundle_created_total
incident_bundle_failed_total
diagnostic_section_failed_total
replay_success_total
replay_mismatch_total
```

---

# 227. Alerting

Alert if:

```text
forensic bundle generation repeatedly fails
diagnostic store fills
replay mismatch on known deterministic operation
```

---

# 228. Audit Metrics

Bundle access logs remain audit events, not metrics.

---

# 229. Correctness Invariants

Add:

## AEQ-INV-DIAG001

```text
Incident diagnostics never become a second source of authoritative business state.
```

## AEQ-INV-DIAG002

```text
Private keys, authentication secrets, and raw secret values are never included in ordinary incident bundles.
```

## AEQ-INV-DIAG003

```text
Every incident bundle declares its schema version, selectors, completeness, and cryptographic content digest.
```

## AEQ-INV-DIAG004

```text
A reproduced domain execution may emit simulated side-effect intents but never performs real external side effects.
```

## AEQ-INV-DIAG005

```text
Diagnostic collection is bounded by explicit size/time/record limits.
```

## AEQ-INV-DIAG006

```text
Forensic explanations distinguish authoritative evidence from derived or best-effort evidence.
```

---

# 230. Additional Invariants

## AEQ-INV-DIAG007

```text
Incident bundle generation applies governance/redaction policy before artifact publication.
```

## AEQ-INV-DIAG008

```text
A bundle marked verified has successfully passed all required hash, schema, and signature checks for its diagnostic mode.
```

## AEQ-INV-DIAG009

```text
A diagnostic replay cannot mutate production authority or production client state.
```

---

# 231. Tests — Secret Redaction

Seed:

```text
access token
API key
private key
password
```

Ensure bundle contains none.

---

# 232. PII Redaction Test

Classify field restricted.

Bundle SupportMinimal:

```text
digest/redacted only
```

---

# 233. Bundle Integrity Test

Modify one file after creation.

Expected:

```text
verification fails
```

---

# 234. Signature Test

Forensic bundle signed with trusted key.

Expected:

```text
verify
```

---

# 235. Archive Bomb Test

Malicious uploaded bundle.

Expected:

```text
size/path limits reject
```

---

# 236. Partial Collection Test

Log backend unavailable.

Expected:

```text
bundle records missing Logs section
```

not silent success.

---

# 237. Operation Explanation Test

Simulate commit + lost response.

Expected conclusion:

```text
TransportLossAfterCommit
```

with authoritative ledger evidence.

---

# 238. Divergence Test

Client digest differs.

Expected bundle:

```text
partition path
expected root
actual root
repair plan ref
```

---

# 239. Epoch Fork Test

Two same-epoch checkpoints differ.

Expected:

```text
ForkDetected
```

with signed checkpoint evidence.

---

# 240. Job Ambiguity Test

Provider timeout after effect.

Bundle should include:

```text
attempt
idempotency key digest/ref
ambiguous policy
reconciliation state
```

---

# 241. Replay Test

Captured deterministic operation replay.

Expected:

```text
same plan digest
```

---

# 242. Replay Mismatch Test

New handler version changes result.

Expected:

```text
mismatch explicitly reported
```

---

# 243. Client Process Kill Test

Bundle after restart should still show:

```text
last durable cursor
outbox state
bootstrap checkpoint
```

---

# 244. Cross-Adapter Test

Create equivalent incident on:

```text
Stoolap
SQLite
Postgres
```

Canonical diagnostic schema should remain comparable.

---

# 245. Load Test

Generate many concurrent support bundle requests.

Part 18 should bound work.

---

# 246. Retention Test

Bundle expires.

Expected:

```text
artifact removed
registry updated
```

---

# 247. Governance Test

Erasure request targets subject referenced in bundle.

Expected:

```text
bundle included in erasure plan
```

unless held.

---

# 248. Module Layout

Suggested:

```text
aequora-diagnostics/
├── incident.rs
├── selector.rs
├── event.rs
├── section.rs
├── policy.rs
├── sanitizer.rs
├── timeline.rs
├── inventory.rs
├── bundle.rs
├── verify.rs
└── errors.rs
```

---

# 249. Replay Integration

```text
aequora-diagnostics/
└── replay/
    ├── manifest.rs
    ├── harness.rs
    ├── simulator.rs
    ├── minimize.rs
    └── report.rs
```

---

# 250. Client Integration

```text
aequora-client/
└── diagnostics/
    ├── ring.rs
    ├── snapshot.rs
    ├── bundle.rs
    └── export.rs
```

---

# 251. Server Integration

```text
aequora-server/
└── diagnostics/
    ├── collector.rs
    ├── operation.rs
    ├── entity.rs
    ├── scope.rs
    ├── job.rs
    ├── authority.rs
    └── admin.rs
```

---

# 252. Adapter Integration

```text
aequora-adapter-sdk/
└── diagnostics.rs
```

---

# 253. CLI Integration

```text
aequora-cli/
└── incident/
    ├── create.rs
    ├── inspect.rs
    ├── verify.rs
    ├── explain.rs
    ├── replay.rs
    └── minimize.rs
```

---

# 254. Admin API

Suggested:

```text
POST /api/admin/v1/incidents/bundles/plan
POST /api/admin/v1/incidents/bundles
GET  /api/admin/v1/incidents/bundles/{id}
GET  /api/admin/v1/forensics/operations/{id}
GET  /api/admin/v1/forensics/jobs/{id}
```

---

# 255. Client Support API

Optional authenticated endpoint:

```text
POST /support/bundles
```

for uploading client bundle.

---

# 256. Upload Authorization

Bind bundle to:

```text
tenant
device
support case
```

---

# 257. Bundle Merge Service

Can merge by:

```text
IncidentId
OperationId
CorrelationId
```

---

# 258. Diagnostic Query Budget

Forensics queries may be expensive.

Use:

```text
read limits
time windows
pagination
```

---

# 259. Deep Historical Query

Make durable background job if very large.

---

# 260. Operational Runbook

When incident occurs:

```text
1. Identify OperationId/ScopeId/JobId.
2. Use explain command.
3. Create SupportMinimal bundle.
4. Escalate to Standard/Forensic only if needed.
5. Reproduce in sandbox.
6. Minimize failing trace.
7. Add regression test.
```

---

# 261. Why Progressive Collection Matters

Most issues can be explained from:

```text
IDs
versions
cursor
ledger
journal
```

No need to export sensitive payload.

---

# 262. Example Sync Incident Workflow

```text
user reports missing update
↓
find OperationId
↓
ledger says committed seq 2201
↓
client cursor 2198
↓
client diagnostic ring shows disk-full during reconcile
↓
root cause confirmed
```

---

# 263. Example Bootstrap Incident

```text
snapshot manifest valid
chunk 17 hash valid
local install checkpoint stuck
local storage critical
```

Conclusion:

```text
resource-constrained install failure
```

---

# 264. Example Compatibility Incident

```text
client supports protocol 4
server minimum = 5
```

Conclusion:

```text
UpgradeRequired
```

not transport failure.

---

# 265. Example Authority Incident

```text
client highest epoch 9
server advertises epoch 8
```

Conclusion:

```text
AuthorityRollbackDetected
```

---

# 266. Example Payment Incident

```text
side-effect intent committed
provider request timed out
provider lookup shows captured
new domain result operation missing
```

Conclusion:

```text
provider result reconciliation incomplete
```

---

# 267. Example Divergence Incident

```text
scope root differs
partition 4/7 differs
single entity digest mismatch
server version 12
client version 11
```

Repair can target one entity.

---

# 268. Completion Criteria

Part 25 is complete when:

```text
[ ] IncidentId/bundle model defined
[ ] operation/entity/scope/device/job forensic views defined
[ ] client diagnostic ring defined
[ ] bundle schema/manifest defined
[ ] privacy redaction policy defined
[ ] runtime/version inventory defined
[ ] trace/log correlation defined
[ ] replay integration defined
[ ] integrity/fork evidence defined
[ ] bundle hashing/signing/encryption defined
[ ] admin/CLI workflows defined
[ ] bounded collection rules defined
[ ] retention/governance rules defined
[ ] parser security rules defined
[ ] reproduction/minimization workflow defined
[ ] diagnostic correctness invariants added
```

---

# 269. Final Architecture

```text
                    INCIDENT SIGNAL
                         │
                         ▼
             OperationId / ScopeId / JobId
                         │
                         ▼
                 Diagnostic Planner
                         │
           ┌─────────────┼─────────────┐
           ▼             ▼             ▼
        Client         Server       Adapters
       Ring/Outbox   Ledger/Journal  Health
           │             │             │
           └─────────────┼─────────────┘
                         ▼
                    Sanitization
                         │
                         ▼
                Canonical Timeline
                         │
                         ▼
                 Incident Bundle
              manifest + hashes + refs
                         │
              encrypt / optional sign
                         │
                         ▼
                   Verification
                         │
                         ▼
                  Replay Sandbox
                         │
                         ▼
                 Minimal Reproducer
                         │
                         ▼
                  Regression Test
```

The architectural principle is:

> **Aequora diagnostics should explain incidents from durable evidence, not from guesswork or oversized raw dumps.**

By correlating OperationIds, cursors, ledger entries, journal sequences, job attempts, authority epochs, integrity roots, runtime versions, and deterministic replay inputs into a bounded and privacy-aware bundle, Aequora can turn difficult synchronization failures into reproducible engineering artifacts rather than one-off production mysteries.
