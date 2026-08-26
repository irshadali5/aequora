# Aequora Sync — Part 24

# Operational Control Plane and Admin API Architecture

## 1. Purpose

Aequora now has many operational subsystems:

```text
authority epochs and failover
background jobs and workflows
snapshots and bootstrap
anti-entropy and repair
audit and provenance
governance and erasure
compatibility policy
crypto key registries
regional read replicas
admission/load control
```

Production operators need a safe way to:

```text
inspect
plan
trigger
pause
resume
verify
recover
```

these systems.

Without a dedicated control plane, teams usually fall back to:

```text
direct SQL
manual file edits
ad-hoc scripts
privileged internal endpoints
```

That is fragile and dangerous.

The central rule is:

> **Operational actions must be explicit, typed, authorized, auditable, fenced, and routed through the same correctness boundaries as ordinary system behavior.**

---

# 2. Goals

The control plane should provide:

```text
admin API
operator CLI
read-only diagnostics
maintenance commands
safe destructive actions
authority management
job management
snapshot management
governance operations
compatibility policy management
crypto metadata inspection
regional status
configuration validation
```

---

# 3. Non-Goals

The control plane is not:

```text
a second business API
a bypass around domain handlers
a generic SQL console
a shell on production hosts
```

---

# 4. Plane Separation

Aequora should distinguish:

```text
Data Plane
Control Plane
Management Plane
```

---

# 5. Data Plane

Handles:

```text
normal sync
client operations
pull/push
bootstrap transfer
live hints
```

---

# 6. Control Plane

Handles:

```text
authority
jobs
snapshots
repair
maintenance
governance
compatibility
```

---

# 7. Management Plane

Optional higher-level deployment tooling:

```text
tenant provisioning
billing integration
cluster deployment
infrastructure configuration
```

Part 24 focuses on the operational control plane.

---

# 8. Separate API Surface

Recommended:

```text
/api/admin/v1/*
```

or entirely separate listener.

---

# 9. Separate Listener

High-assurance deployments should consider:

```text
public data listener
private admin listener
```

Example:

```text
0.0.0.0:443 data plane
127.0.0.1/private-net:9443 admin
```

---

# 10. Network Restriction

Admin plane may be limited to:

```text
private network
VPN
mTLS
localhost
zero-trust gateway
```

---

# 11. Authentication

Admin authentication must be stronger than ordinary user auth.

Potential:

```text
OIDC admin identity
mTLS
hardware-backed credentials
service identity
```

---

# 12. Authorization Model

Use explicit permissions.

Examples:

```text
Authority.View
Authority.Promote
Jobs.View
Jobs.Retry
Snapshot.Build
Governance.Execute
Crypto.Rotate
Compatibility.Update
Diagnostics.Export
```

---

# 13. No "Super Admin Does Everything" Assumption

Support least privilege.

---

# 14. Role Examples

```text
ReadOnlyOperator
SupportEngineer
SRE
SecurityAdmin
GovernanceAdmin
ReleaseAdmin
```

---

# 15. Separation of Duties

High-risk actions may require two-person approval.

Examples:

```text
force authority promotion
legal hold release
mass erasure
key destruction
```

---

# 16. AdminOperationId

Define:

```rust
pub struct AdminOperationId(Uuid);
```

Every state-changing admin action gets stable identity.

---

# 17. AdminAction Envelope

```rust
pub struct AdminAction<T> {
    pub admin_operation_id: AdminOperationId,
    pub actor: AdminPrincipal,
    pub reason: AdminReasonCode,
    pub payload: T,
}
```

---

# 18. Reason Required

For high-risk actions:

```text
reason code
+
optional human note
```

should be mandatory.

---

# 19. Audit

Every privileged mutation generates Part 13 administrative/security audit.

---

# 20. Idempotency

Admin write endpoints also need idempotency.

Retrying:

```text
promote
pause
retry job
create hold
```

must not accidentally duplicate action.

---

# 21. Control Plane Command Model

Prefer typed commands:

```text
PromoteAuthority
RetryJob
BuildSnapshot
CreateLegalHold
RotateKey
SetCompatibilityPolicy
```

---

# 22. Command Handler

Admin command should execute through dedicated application service.

Never directly update metadata table from Axum handler.

---

# 23. Axum Admin Route

Thin:

```text
decode
authenticate
authorize
validate
submit admin command
return typed result
```

---

# 24. Read Models

Admin reads can use dedicated projections.

Examples:

```text
job dashboard
replica status
authority status
queue health
```

---

# 25. Authority Status API

Example:

```text
GET /api/admin/v1/authority/status
```

returns:

```text
AuthorityId
epoch
role
fence
current sequence
promotion readiness
```

---

# 26. Authority Promote API

```text
POST /api/admin/v1/authority/promote
```

requires:

```text
promotion class
fencing confirmation
reason
approval token if required
```

---

# 27. Force Promotion

Separate endpoint/action.

Never overload normal promote with hidden force flag.

---

# 28. Authority Verify

Runs:

```text
journal checkpoint
ledger consistency
audit checkpoint
governance state
```

---

# 29. Job APIs

Read:

```text
GET /jobs
GET /jobs/{id}
```

Mutate:

```text
POST /jobs/{id}/retry
POST /jobs/{id}/cancel
POST /jobs/{id}/quarantine
```

---

# 30. Job Retry Safety

Retry command must check job kind semantics.

Admin cannot force unsafe retry of ambiguous payment blindly.

---

# 31. Job Reconciliation Action

High-risk jobs may require:

```text
reconcile
```

instead of retry.

---

# 32. Snapshot APIs

```text
GET /snapshots
POST /snapshots/build
POST /snapshots/{id}/verify
POST /snapshots/{id}/expire
```

---

# 33. Snapshot Build

Creates durable Part 23 job.

Admin request returns:

```text
JobId
```

---

# 34. Anti-Entropy APIs

```text
POST /integrity/scan
POST /integrity/repair-plan
POST /integrity/repair
```

---

# 35. Repair Safety

Repair must preserve authoritative direction.

Admin cannot choose client state as authority unless explicit disaster import workflow.

---

# 36. Governance APIs

```text
POST /retention/plan
POST /retention/execute
POST /legal-holds
POST /erasure/plan
POST /erasure/execute
```

---

# 37. Dry Run

Destructive endpoints should support plan-first model.

---

# 38. Plan/Execute Separation

Recommended pattern:

```text
POST /.../plan
↓
PlanId
↓
review
↓
POST /.../execute/{PlanId}
```

---

# 39. PlanId

```rust
pub struct PlanId(Uuid);
```

---

# 40. Plan Digest

Persist:

```text
plan digest
```

so execution proves it used reviewed plan.

---

# 41. Plan Expiry

Plans should expire after configuration/state changes.

---

# 42. Stale Plan

If critical state changed:

```text
execution rejected
replan required
```

---

# 43. Compatibility APIs

```text
GET /compatibility
POST /compatibility/policy
POST /compatibility/feature-gates
```

---

# 44. Compatibility Validation

Before activation:

```text
check fleet capabilities
client distribution
required nodes
```

---

# 45. Crypto APIs

Safe read:

```text
GET /crypto/keys
GET /crypto/policy
```

Mutate:

```text
POST /crypto/rotate
POST /crypto/revoke
```

---

# 46. Never Return Private Key

Admin API only returns:

```text
key ID
public key
status
purpose
```

---

# 47. Key Destruction

Separate high-risk action.

Requires governance/security permissions.

---

# 48. Regional APIs

```text
GET /regions
GET /replicas
GET /replicas/{id}/watermark
```

---

# 49. Region Drain

Potential action:

```text
POST /regions/{id}/drain
```

for maintenance.

---

# 50. Drain Meaning

Stop new:

```text
reads
live connections
jobs
```

before shutdown.

---

# 51. Maintenance Mode

Define:

```rust
pub enum MaintenanceMode {
    Normal,
    ReadOnly,
    Drain,
    Recovery,
}
```

---

# 52. ReadOnly

Blocks normal writes.

Useful for:

```text
migration
restore verification
```

---

# 53. Drain

Stops new work while allowing in-flight completion.

---

# 54. Recovery

Part 16 restricted behavior.

---

# 55. Maintenance State Persistence

Store durably if must survive restart.

---

# 56. Emergency Stop

High-assurance control:

```text
stop authoritative writes
```

without shutting process down.

---

# 57. WriteKillSwitch

Could set:

```text
AuthorityRuntimeMode::ReadOnlyVerification
```

or dedicated flag.

---

# 58. Kill Switch Audit

Always audited.

---

# 59. Tenant Controls

Potential:

```text
suspend tenant
read-only tenant
force device logout
rebootstrap tenant scopes
```

---

# 60. Tenant Suspension

Should affect:

```text
authorization
sync writes
jobs
```

according to policy.

---

# 61. Tenant Read-Only

Allow:

```text
reads/export
```

but block mutation.

---

# 62. Device Controls

```text
revoke device
retire device
force rebootstrap
purge scope
```

---

# 63. Device Revocation

Must update authoritative device state and audit.

---

# 64. Force Rebootstrap

Useful after:

```text
client corruption
schema incompatibility
```

---

# 65. Scope Controls

```text
rebuild scope
bump scope generation
inspect membership
```

---

# 66. Scope Generation Bump

High-impact.

Should require explicit plan/reason.

---

# 67. Journal Controls

Read:

```text
current sequence
retention floor
lag
```

Avoid arbitrary deletion API.

---

# 68. Journal Floor Advance

Should happen through retention/governance engine, not direct manual number edit.

---

# 69. Operation Lookup

Admin:

```text
GET /operations/{OperationId}
```

returns:

```text
ledger status
journal sequence
audit refs
lineage
```

---

# 70. Explain Operation

Integrate Part 13:

```text
GET /operations/{id}/explain
```

---

# 71. Conflict Administration

Support:

```text
inspect
```

but manual resolution should still create a domain operation.

---

# 72. No Direct Conflict Row Edit

Admin resolution:

```text
SubmitResolutionOperation
```

---

# 73. Import APIs

```text
POST /imports
GET /imports/{id}
POST /imports/{id}/cutover
```

---

# 74. Import Cutover

Requires plan/readiness check.

---

# 75. Export APIs

```text
POST /exports
GET /exports/{id}
```

Creates Part 23 job.

---

# 76. Diagnostic APIs

```text
GET /health/deep
GET /diagnostics/runtime
POST /diagnostics/bundle
```

---

# 77. Deep Health

May run:

```text
DB connectivity
authority fence
job store
key registry
snapshot store
```

---

# 78. Avoid Deep Health on Public Liveness

Expensive checks should not run every load-balancer probe.

---

# 79. Liveness vs Readiness vs Deep Health

Separate:

```text
liveness = process alive
readiness = can serve
deep = operator diagnostics
```

---

# 80. Runtime Diagnostics

Can expose:

```text
queue depths
pool saturation
load state
worker counts
replica lag
```

---

# 81. Sensitive Diagnostics

Do not expose:

```text
raw payload
PII
secret config
```

by default.

---

# 82. Incident Bundle

Part 25 will define full format.

Admin action can request bundle.

---

# 83. Config APIs

Read effective config.

Changing config via API should be limited.

---

# 84. Immutable Startup Config

Some settings should require restart.

Examples:

```text
database URL
storage backend
trust root
```

---

# 85. Dynamic Policy

Safe dynamic examples:

```text
admission limits
feature gates
compatibility policy
maintenance mode
```

---

# 86. Configuration Source of Truth

Do not let:

```text
RON file
environment
admin DB
```

silently fight.

Define precedence.

---

# 87. Config Precedence

Example:

```text
binary defaults
↓
RON file
↓
environment secrets
↓
dynamic control-plane policy
```

Only selected keys dynamic.

---

# 88. Config Generation

```rust
pub struct ConfigGeneration(u64);
```

Dynamic updates increment generation.

---

# 89. Atomic Config Swap

Validate new config fully.

Then atomically replace runtime snapshot.

---

# 90. Rollback

Keep prior config generation for operator rollback where safe.

---

# 91. Audit Config Changes

Store:

```text
old generation
new generation
actor
reason
```

---

# 92. Admin API Versioning

Admin API itself needs explicit version.

```text
/api/admin/v1
```

---

# 93. Backward Compatibility

CLI and server may be different versions.

Part 21 negotiation principles apply.

---

# 94. Admin Capability Discovery

CLI can fetch:

```text
supported admin capabilities
```

---

# 95. CLI

Suggested root:

```text
aequora admin ...
```

---

# 96. CLI Examples

```text
aequora admin authority status
aequora admin jobs list
aequora admin jobs retry <id>
aequora admin snapshot build
aequora admin retention plan
aequora admin compat show
```

---

# 97. CLI Output Formats

Support:

```text
human table
RON
JSON only for interoperability
```

---

# 98. Machine Automation

Exit codes and stable reason codes.

---

# 99. No Parsing Human Text

Scripts should use structured output.

---

# 100. Confirmation

CLI high-risk actions require:

```text
--confirm
```

or interactive confirmation.

---

# 101. Automation Mode

For CI/automation:

```text
--yes --reason-code ...
```

with appropriate credentials.

---

# 102. Approval Workflow

High-risk action:

```text
Requested
↓
AwaitingApproval
↓
Approved
↓
Executing
↓
Completed
```

---

# 103. ApprovalId

```rust
pub struct ApprovalId(Uuid);
```

---

# 104. Approval Record

Fields:

```text
requestor
approver
action digest
expires_at
```

---

# 105. Same Person Restriction

Optional policy:

```text
approver != requestor
```

---

# 106. Approval Scope

Approval binds exact:

```text
action
plan digest
target
```

not generic future authority.

---

# 107. Approval Expiry

Short-lived.

---

# 108. Admin Jobs

Long control-plane work uses Part 23 jobs.

Examples:

```text
snapshot build
export
retention purge
verify metadata
```

---

# 109. Admin Request Does Not Hold HTTP Open

Return:

```text
JobId/AdminOperationId
```

---

# 110. Poll/Live Status

CLI/UI can poll or use admin live stream.

---

# 111. Admin Live Stream

Optional:

```text
SSE/WebSocket
```

for job/status updates.

---

# 112. Admin Live Stream Is Advisory

Durable job state remains truth.

---

# 113. Rate Limiting

Admin endpoints also require rate limits.

---

# 114. Brute Force / Abuse

Stronger auth does not eliminate abuse risk.

---

# 115. Tenant Boundaries

Support operators may be scoped to specific tenant.

---

# 116. Global Admin

Separate role.

---

# 117. Support Impersonation

Avoid user impersonation where possible.

Prefer scoped diagnostic permissions.

---

# 118. Break-Glass Access

Emergency elevated access may exist.

Must be:

```text
time-limited
strongly authenticated
fully audited
```

---

# 119. BreakGlassSessionId

Track distinct session.

---

# 120. Break-Glass Alert

Security alert when used.

---

# 121. Admin Token Lifetime

Short.

---

# 122. mTLS

Good option for machine/admin CLI in private environments.

---

# 123. CSRF

If browser admin UI uses cookies:

```text
CSRF protection required
```

---

# 124. CORS

Admin plane should be restrictive.

---

# 125. Browser Admin UI

Optional Dioxus/web frontend.

Uses same admin API.

---

# 126. No UI-Only Security

Authorization is server-side.

---

# 127. Audit Every Destructive Action

Examples:

```text
job retry
key revoke
device revoke
scope generation bump
tenant purge
```

---

# 128. Read Audit

Sensitive reads may also be audited.

Examples:

```text
export audit records
view private diagnostics
```

---

# 129. Idempotency Key

Admin client sends:

```text
AdminOperationId
```

server deduplicates.

---

# 130. Admin Operation Ledger

Logical:

```text
aequora_admin_operation
```

Fields:

```text
admin_operation_id
actor
action_kind
target
payload_digest
status
created_at
completed_at
result_code
```

---

# 131. Admin Operation Status

```text
Pending
Approved
Executing
Completed
Rejected
Failed
```

---

# 132. Admin Action Digest

Canonical digest prevents retry with changed payload under same ID.

---

# 133. Dangerous Direct Overrides

Avoid:

```text
set journal sequence
set operation committed
edit authority epoch
```

raw APIs.

---

# 134. Recovery Tools

If manual metadata repair absolutely required, provide:

```text
special offline repair command
```

not ordinary admin API.

---

# 135. Offline Repair Mode

Requires:

```text
service stopped/read-only
backup
plan
verification
audit incident
```

---

# 136. Production Safe Defaults

By default:

```text
admin listener disabled publicly
destructive APIs require explicit config
```

---

# 137. Self-Hosted Profile

Could bind admin to:

```text
localhost
```

with CLI via SSH tunnel.

---

# 138. SaaS Profile

Private service/network and central admin auth.

---

# 139. Kubernetes

Admin API can remain ClusterIP/private.

---

# 140. Health Integration

Public:

```text
/live
/ready
```

Admin:

```text
/deep-health
```

---

# 141. Maintenance Window

Control plane can schedule:

```text
read-only
drain
migration
resume
```

---

# 142. MaintenancePlan

```rust
pub struct MaintenancePlan {
    pub maintenance_id: MaintenanceId,
    pub start_at: Timestamp,
    pub mode: MaintenanceMode,
    pub reason: AdminReasonCode,
}
```

---

# 143. Scheduled Maintenance

Part 23 durable timer/job triggers transition.

---

# 144. Client Notification

Part 08 may send:

```text
MaintenanceNotice
```

---

# 145. No Reliance on Notice

Server enforces mode regardless of client knowledge.

---

# 146. Drain Procedure

```text
stop new admin/data writes
↓
stop claiming jobs
↓
allow in-flight completion
↓
close live connections
↓
ready=false
```

---

# 147. Graceful Shutdown

Control plane can expose drain status.

---

# 148. Deployment Hooks

Rolling deployment tooling can call:

```text
drain
verify
resume
```

---

# 149. Migration Coordination

Before metadata migration:

```text
drain old writers
ensure compatible fleet
run migration
resume
```

---

# 150. Authority Guard

Admin actions that mutate authority state must pass Part 16 fencing.

---

# 151. Job Guard

Job actions pass Part 23 state/lease invariants.

---

# 152. Governance Guard

Destructive governance actions validate legal holds.

---

# 153. Crypto Guard

Key actions validate key purpose/state.

---

# 154. Compatibility Guard

Feature gate cannot be made Required if fleet lacks support.

---

# 155. Regional Guard

Replica drain/promotion obey residency and authority rules.

---

# 156. Control Plane Transactionality

Small state changes:

```text
single DB transaction
```

Large workflows:

```text
durable job
```

---

# 157. Admin Action Lifecycle

```text
Received
↓
Authenticated
↓
Authorized
↓
Validated
↓
Planned
↓
Approved if required
↓
Executed
↓
Verified
↓
Audited
```

---

# 158. Verification

High-risk action should not return Completed until postcondition verified.

---

# 159. Example: Device Revoke

Verify:

```text
device status revoked
credentials invalidated
live connection closed or expiry ensured
```

---

# 160. Example: Snapshot Build

Verify:

```text
manifest
chunks
root digest
publication
```

---

# 161. Example: Key Rotation

Verify:

```text
new key active
old key verification-only
registry generation updated
```

---

# 162. Example: Authority Promotion

Verify:

```text
old primary fenced
new primary serving
epoch correct
```

---

# 163. Control Plane Read Consistency

Operational reads about authority/job state should usually use:

```text
Authority
```

or sufficiently fresh primary metadata.

Do not show stale critical control status from lagging replica.

---

# 164. Eventual Dashboard Reads

Charts/historical metrics may be eventual.

---

# 165. Command Routing

All mutating admin commands route to active authority/control-plane leader.

---

# 166. Multi-Region Admin

Nearest read-only admin view possible.

Mutations go authority.

---

# 167. Control Plane Availability

If control plane unavailable, data plane may continue normal operation.

---

# 168. Independence

Do not make every sync request depend on admin service availability.

---

# 169. Emergency Dependency

Some policies may be cached in data plane:

```text
maintenance mode
compat policy
```

with durable snapshot.

---

# 170. Policy Distribution

Control-plane update publishes:

```text
new config generation
```

data nodes reload.

---

# 171. Eventual Policy Propagation

For safety-sensitive policy:

```text
do not report complete until required nodes acknowledge
```

or enforce via central authority DB.

---

# 172. Node Acknowledgement

Optional:

```text
node_id
config_generation
last_seen
```

---

# 173. Fleet Status

Control plane can show:

```text
which nodes applied generation
```

---

# 174. Metrics

```text
admin_requests_total
admin_denied_total
admin_operation_failed_total
maintenance_mode
break_glass_active
config_generation
```

---

# 175. Audit Metrics

Avoid high-cardinality action target IDs.

---

# 176. Logs

Structured:

```text
admin_action_requested
admin_action_approved
admin_action_completed
break_glass_started
maintenance_entered
```

---

# 177. Alerting

Alert on:

```text
break-glass access
force promotion
key destruction
mass tenant purge
control-plane auth failures
```

---

# 178. Tracing

Every admin action trace includes:

```text
AdminOperationId
actor class
action kind
```

---

# 179. Sensitive Payload Redaction

Admin payloads may contain:

```text
reason
target IDs
```

but not secret values.

---

# 180. API Error Taxonomy

Examples:

```text
AdminUnauthorized
AdminForbidden
ApprovalRequired
PlanExpired
PlanStale
UnsafeOperation
AuthorityNotPrimary
JobNotRetryable
LegalHoldBlocks
FleetIncompatible
```

---

# 181. No Generic 500 for Policy Failure

Return precise machine code.

---

# 182. HTTP Status

Typical:

```text
401 auth
403 permission
409 state conflict
412 precondition failed
422 invalid plan/action
429 rate limit
503 dependency unavailable
```

---

# 183. ETags / Preconditions

For mutable policy resources:

```text
If-Match generation
```

or explicit expected generation.

Prevents lost admin updates.

---

# 184. Optimistic Admin Update

Example:

```text
compatibility policy generation 10
```

update expects 10.

If current 11:

```text
reject
```

---

# 185. Plan Version

Plan binds:

```text
policy generation
authority epoch
target snapshot
```

---

# 186. Plan Staleness Check

If any safety-critical dependency changed:

```text
replan
```

---

# 187. Admin UI Design

Dashboard sections:

```text
Overview
Authority
Jobs
Snapshots
Integrity
Governance
Compatibility
Crypto
Regions
Diagnostics
```

---

# 188. Overview

Show:

```text
health
authority epoch
load state
job backlog
replica lag
latest snapshot
```

---

# 189. Dangerous Action UI

Use:

```text
clear impact
reason
plan preview
approval status
```

not one-click destructive buttons.

---

# 190. No Dark Patterns

Operational UI should favor clarity over speed for destructive actions.

---

# 191. Support Workflow

Support engineer can:

```text
lookup OperationId
inspect lineage
inspect client status
```

without permission to promote authority or destroy keys.

---

# 192. SRE Workflow

SRE can:

```text
drain node
inspect load
retry safe infra jobs
```

---

# 193. Security Workflow

Security admin can:

```text
revoke device
rotate key
inspect audit integrity
```

---

# 194. Governance Workflow

Governance admin can:

```text
plan erasure
create legal hold
execute retention
```

---

# 195. Release Workflow

Release admin can:

```text
canary feature
update compatibility policy
```

---

# 196. Control Plane Storage

Logical records:

```text
aequora_admin_operation
aequora_admin_approval
aequora_maintenance_state
aequora_dynamic_config
aequora_node_status
```

---

# 197. `aequora_admin_operation`

Fields:

```text
admin_operation_id
actor_id
action_kind
target_kind
target_id
payload_digest
state
reason_code
created_at
completed_at
```

---

# 198. `aequora_admin_approval`

Fields:

```text
approval_id
admin_operation_id
approver
action_digest
state
expires_at
```

---

# 199. `aequora_dynamic_config`

Fields:

```text
generation
config_kind
payload
digest
created_by
created_at
```

---

# 200. `aequora_maintenance_state`

Fields:

```text
mode
generation
reason_code
effective_at
```

---

# 201. `aequora_node_status`

Operational/ephemeral:

```text
node_id
region
build
capabilities
config_generation
last_seen
```

---

# 202. Node Status Is Not Authority Truth

Use for fleet visibility only.

---

# 203. Control Plane Jobs

Large admin action creates:

```text
JobId
```

linked to:

```text
AdminOperationId
```

---

# 204. Linkage

```text
AdminOperationId
→
PlanId
→
JobId(s)
→
AuditEventId
```

---

# 205. Explain Admin Action

Part 13 can reconstruct:

```text
who requested
who approved
what plan
which jobs
what result
```

---

# 206. Retention

Admin operation records may need long retention.

Depends on class.

---

# 207. Erasure

Admin records referencing erased subject should preserve only necessary pseudonymous IDs.

---

# 208. Backup

Control-plane durable policy/state must be backed up with authority metadata.

---

# 209. Restore

After PITR, Part 16/14 restore reconciliation runs before control plane enables dangerous actions.

---

# 210. Startup

Admin API startup validates:

```text
auth provider
permission registry
authority state
dynamic config schema
```

---

# 211. Fail Closed

If admin auth misconfigured:

```text
admin listener does not start
```

or mutating endpoints disabled.

---

# 212. Data Plane Independence

Public sync may still start if safe.

---

# 213. Read-Only Emergency Mode

If admin dependencies partially unavailable, keep read-only diagnostic endpoints if authenticated safely.

---

# 214. API Documentation

Generate OpenAPI only for admin JSON interoperability if needed.

Core internal client may use Postcard.

---

# 215. Admin Wire Format

For CLI/internal Rust clients:

```text
Postcard
```

can be primary.

For browser/external tools:

```text
JSON
```

may be appropriate.

---

# 216. JSON Boundary

Admin API is a reasonable JSON surface because:

```text
human tooling
curl
browser
third-party ops
```

This is one of the cases where JSON is justified.

---

# 217. Typed Internal Core

JSON DTO maps immediately into typed admin commands.

Do not carry `serde_json::Value` into core.

---

# 218. Versioned Admin DTO

Admin request/response schemas also versioned.

---

# 219. CLI Compatibility

CLI checks server admin API version/capabilities.

---

# 220. Automation Integrations

Future:

```text
Terraform
Ansible
Kubernetes operator
```

may call admin API.

---

# 221. Declarative Desired State

Some controls can be declarative:

```text
compatibility policy
admission config
maintenance schedule
```

---

# 222. Imperative Actions

Some are inherently commands:

```text
promote authority
retry job
destroy key
```

---

# 223. Do Not Force Everything Into CRUD

Control plane should model commands as commands.

---

# 224. Reconciliation Loop

For declarative resources:

```text
desired state
↓
controller
↓
actual state
```

Could be future enhancement.

---

# 225. No Kubernetes-Style Complexity Initially

Use direct validated updates first.

---

# 226. Safety Invariants

Add:

## AEQ-INV-ADMIN001

```text
No mutating admin endpoint may bypass the authoritative application/domain service responsible for the affected invariant.
```

## AEQ-INV-ADMIN002

```text
Every high-risk admin mutation is attributable to an authenticated principal and durable AdminOperationId.
```

## AEQ-INV-ADMIN003

```text
Retrying the same AdminOperationId with a different action payload is rejected.
```

## AEQ-INV-ADMIN004

```text
Destructive plan-based operations execute only the exact reviewed plan digest and fail when that plan becomes stale.
```

## AEQ-INV-ADMIN005

```text
Admin authorization is evaluated server-side and is never inferred from UI visibility.
```

## AEQ-INV-ADMIN006

```text
Private cryptographic key material is never returned through the admin API.
```

---

# 227. Additional Invariants

## AEQ-INV-ADMIN007

```text
A force/override operation is a distinct action with stronger authorization and audit semantics than its normal counterpart.
```

## AEQ-INV-ADMIN008

```text
Control-plane unavailability does not automatically invalidate normal data-plane correctness or availability.
```

## AEQ-INV-ADMIN009

```text
High-risk actions are not reported complete until their defined postconditions are verified.
```

---

# 228. Tests — Authorization

Attempt every admin action with:

```text
no auth
wrong role
tenant-scoped operator
global operator
```

---

# 229. Idempotency Test

Repeat:

```text
same AdminOperationId + same payload
```

Expected:

```text
same result
```

Different payload:

```text
rejected
```

---

# 230. Plan Stale Test

Create retention plan.

Change legal hold.

Execute plan.

Expected:

```text
rejected, replan
```

---

# 231. Approval Test

Requester attempts own approval when separation required.

Expected:

```text
denied
```

---

# 232. Force Promotion Test

Normal SRE role:

```text
denied
```

Break-glass/authorized role:

```text
allowed + critical audit
```

---

# 233. Key API Test

Ensure no endpoint serializes private key bytes.

---

# 234. Job Retry Test

Ambiguous payment job.

Generic retry action:

```text
rejected
```

Reconciliation action required.

---

# 235. Drain Test

Enter drain.

Expected:

```text
no new work
existing work finishes/checkpoints
readiness eventually false
```

---

# 236. Config CAS Test

Two admins edit same generation.

One succeeds.

Other gets conflict.

---

# 237. Browser Security Test

Validate:

```text
CSRF
CORS
session timeout
```

if web admin enabled.

---

# 238. Audit Test

Every destructive admin action creates expected audit chain.

---

# 239. Chaos Test

Admin control plane crashes during long purge.

Durable JobId allows continuation.

---

# 240. Multi-Node Test

Two admin nodes receive same idempotent action.

Only one logical action occurs.

---

# 241. Recommended Modules

```text
aequora-admin/
├── command.rs
├── auth.rs
├── permission.rs
├── idempotency.rs
├── plan.rs
├── approval.rs
├── maintenance.rs
├── config.rs
├── errors.rs
└── audit.rs
```

---

# 242. Server Integration

```text
aequora-server/
└── admin/
    ├── routes.rs
    ├── dto.rs
    ├── middleware.rs
    ├── authority.rs
    ├── jobs.rs
    ├── snapshots.rs
    ├── governance.rs
    ├── compatibility.rs
    ├── crypto.rs
    └── diagnostics.rs
```

---

# 243. CLI Crate

```text
aequora-cli/
└── admin/
    ├── authority.rs
    ├── jobs.rs
    ├── snapshot.rs
    ├── governance.rs
    ├── compat.rs
    ├── crypto.rs
    └── diagnostics.rs
```

---

# 244. AdminService

High-level facade:

```rust
pub struct AdminService {
    authority: AuthorityAdmin,
    jobs: JobAdmin,
    snapshots: SnapshotAdmin,
    governance: GovernanceAdmin,
    compatibility: CompatibilityAdmin,
    crypto: CryptoAdmin,
}
```

---

# 245. Permission Registry

Stable numeric permission IDs.

Avoid free-form strings as canonical persistence key.

---

# 246. AdminActionKind

Stable numeric registry.

---

# 247. ReasonCode Registry

Stable codes for:

```text
planned maintenance
incident response
customer request
security compromise
migration
```

---

# 248. Policy Engine

Authorization can integrate existing application RBAC/ABAC.

Aequora defines required context.

---

# 249. AuthContext

Admin auth context includes:

```text
principal
tenant scope
roles/permissions
session assurance
break-glass status
```

---

# 250. Step-Up Authentication

High-risk action may require stronger recent auth.

---

# 251. Session Assurance Level

Concept:

```rust
pub enum AssuranceLevel {
    Normal,
    MFA,
    HardwareBacked,
    BreakGlass,
}
```

---

# 252. Action Requirements

Example:

```text
view jobs -> Normal
rotate key -> MFA
destroy key -> HardwareBacked/approval
```

---

# 253. Avoid Hardcoding Authentication Vendor

Core works with generic assurance/context.

---

# 254. Control Plane SLO

Admin plane can have lower throughput but high correctness.

---

# 255. Latency

Most admin actions can take seconds/minutes asynchronously.

No need to optimize like sync hot path.

---

# 256. Availability

Read-only operational status should be highly available.

Destructive actions may intentionally require authority region.

---

# 257. Completion Criteria

Part 24 is complete when:

```text
[ ] control/data plane separation defined
[ ] separate admin API/listener model defined
[ ] strong auth/permission model defined
[ ] AdminOperationId/idempotency defined
[ ] authority/job/snapshot/governance APIs defined
[ ] crypto/compat/regional controls defined
[ ] plan/execute workflow defined
[ ] approval/separation-of-duties defined
[ ] maintenance/drain/recovery modes defined
[ ] dynamic config generation/CAS defined
[ ] admin operation persistence defined
[ ] CLI/UI integration defined
[ ] break-glass semantics defined
[ ] audit requirements defined
[ ] admin correctness/security tests defined
[ ] admin invariants added
```

---

# 258. Final Architecture

```text
                        OPERATOR / SRE / ADMIN
                                  │
                                  ▼
                         Strong Authentication
                                  │
                                  ▼
                          Admin Authorization
                                  │
                                  ▼
                         ADMIN CONTROL PLANE
                                  │
               ┌──────────────────┼──────────────────┐
               ▼                  ▼                  ▼
          Authority           Jobs/Workflows      Governance
               │                  │                  │
               ▼                  ▼                  ▼
          Snapshots          Compatibility         Crypto
               │                  │                  │
               └──────────────────┼──────────────────┘
                                  ▼
                        Typed Admin Commands
                                  │
                                  ▼
                      Correctness Application Layer
                                  │
                                  ▼
                         Durable State / Jobs
                                  │
                                  ▼
                           Verification + Audit

Data plane remains separate:

Client
  │
  ▼
Sync API
  │
  ▼
Domain Execution
```

The architectural principle is:

> **Aequora's control plane should make privileged operations safer than direct database access, not merely more convenient.**

By separating operational commands from the data plane, requiring typed authorization and idempotency, using plan/approval workflows for destructive actions, and routing every action through the same authority, governance, job, crypto, and audit invariants, Aequora can be operated safely in both small self-hosted deployments and large enterprise environments.
