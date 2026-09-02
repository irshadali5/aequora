# Aequora Sync — Part 46

# Observability Implementation, Metrics, Distributed Tracing, Structured Logging, SLOs, Alerting, and Production Telemetry Architecture

## 1. Purpose

Aequora needs observability that can answer not only:

> “Is the server up?”

but also:

```text
Why is this operation still pending?
Where is sync latency being spent?
Is PostgreSQL saturated?
Did a retry duplicate an effect?
Which authority epoch produced this event?
Why did conflicts suddenly increase?
Are clients failing to bootstrap?
Is one tenant consuming disproportionate capacity?
Did a release introduce a regression?
Can an incident be reconstructed without exposing private data?
```

The central rule is:

> **Observability must make correctness and production behavior explainable without becoming part of correctness itself.**

If the telemetry backend disappears, Aequora continues operating safely.

---

# 2. Observability Layers

Separate five concepts:

```text
Metrics
Structured Logs
Distributed Traces
Durable Audit
Diagnostics / Forensics
```

They overlap operationally but have different contracts.

---

# 3. Metrics

Metrics answer aggregate questions:

```text
How many?
How fast?
How often?
How full?
How far behind?
```

Examples:

```text
sync exchanges/sec
p95 exchange latency
DB pool wait
journal append rate
pending jobs
bootstrap failures
```

---

# 4. Structured Logs

Logs describe discrete operational events:

```text
server startup
config reload
operation rejected
DB retry
snapshot published
worker lease lost
authority mismatch
```

Logs are not the business audit ledger.

---

# 5. Distributed Tracing

Traces explain one execution path across:

```text
HTTP
server core
domain handler
PostgreSQL
job creation
external provider
```

They answer:

```text
where did time go?
```

and:

```text
which components participated?
```

---

# 6. Durable Audit

Audit answers:

```text
who performed a meaningful business/admin action?
what changed?
why?
under which authority and policy?
```

Audit belongs to authoritative durable state and must not be replaced by logs or traces.

---

# 7. Diagnostics / Forensics

Diagnostics combine bounded evidence:

```text
version
configuration digest
registry digest
operation state
safe logs
trace IDs
authority state
store metadata
```

for incident investigation.

Part 25 remains authoritative for incident bundles.

---

# 8. Crate Architecture

Recommended:

```text
aequora-observability
aequora-metrics
aequora-tracing
aequora-diagnostics
```

or a smaller initial split:

```text
aequora-observability
aequora-diagnostics
```

Do not over-fragment before APIs stabilize.

---

# 9. Dependency Direction

```text
domain/core
   |
   v
observability abstractions
   |
   v
runtime exporters
```

Core logic may emit semantic telemetry events but should not depend on:

```text
Prometheus server
OpenTelemetry collector
Grafana
vendor SDK
```

---

# 10. Rust Implementation Direction

A practical Rust stack can use:

```text
tracing
tracing-subscriber
metrics abstraction
OpenTelemetry-compatible exporters where desired
```

Exact exporter crates remain replaceable.

---

# 11. Telemetry Provider Interfaces

Concept:

```rust
pub trait MetricsSink {
    fn counter(&self, metric: MetricId, value: u64, attrs: &Attributes);
    fn histogram(&self, metric: MetricId, value: f64, attrs: &Attributes);
}

pub trait DiagnosticEventSink {
    fn emit(&self, event: DiagnosticEvent);
}
```

Hot paths should avoid expensive dynamic allocation where practical.

---

# 12. Telemetry Is Best-Effort

A telemetry export failure must not roll back:

```text
Tx A
Tx B
Tx C
```

or authoritative domain work.

---

# 13. Bounded Telemetry

Every telemetry queue must be bounded.

Under overload:

```text
drop/coalesce low-value telemetry
```

rather than exhaust memory.

---

# 14. Telemetry Priority

Possible:

```text
CriticalSecurity
OperationalError
OperationalInfo
Debug
Verbose
```

Critical security/audit events that require durability belong in durable audit/security stores, not an in-memory telemetry queue.

---

# 15. Semantic Instrumentation

Instrument around stable Aequora concepts:

```text
OperationId
OperationKind
EntityType
TenantId
ScopeId
AuthorityId
AuthorityEpoch
TimelineSequence
JobKind
AdapterId
```

but not all belong as metric labels.

---

# 16. Cardinality Rule

Never place unbounded IDs into metric labels.

Bad:

```text
operation_id
user_id
device_id
entity_id
request_id
```

Good:

```text
operation_kind
result_class
adapter
route
environment
region
```

---

# 17. Tenant Metric Cardinality

Per-tenant metrics can become dangerously high-cardinality.

Default global telemetry should avoid `TenantId` labels.

Use:

```text
top-N heavy tenants
sampled tenant diagnostics
control-plane tenant query
```

for detailed investigation.

---

# 18. Logs Can Carry IDs

Structured logs may contain safe identifiers such as:

```text
OperationId
RequestId
CorrelationId
```

subject to privacy policy.

This is different from metric labels.

---

# 19. Trace Attributes

Traces may carry selected identifiers but must still obey:

```text
privacy
sampling
cardinality
export policy
```

---

# 20. Stable Metric Names

Treat important metric names as operational contracts.

Recommended prefix:

```text
aequora_
```

---

# 21. Metric Categories

```text
aequora_http_*
aequora_sync_*
aequora_operation_*
aequora_db_*
aequora_journal_*
aequora_ledger_*
aequora_client_*
aequora_outbox_*
aequora_bootstrap_*
aequora_snapshot_*
aequora_job_*
aequora_consumer_*
aequora_authority_*
aequora_security_*
aequora_runtime_*
```

---

# 22. HTTP Metrics

Examples:

```text
aequora_http_requests_total
aequora_http_request_duration_seconds
aequora_http_request_body_bytes
aequora_http_response_body_bytes
aequora_http_inflight_requests
aequora_http_rejected_total
```

Labels:

```text
route
method
status_class
result_class
```

Do not label raw URLs.

---

# 23. Sync Metrics

Examples:

```text
aequora_sync_exchange_total
aequora_sync_exchange_duration_seconds
aequora_sync_push_operations
aequora_sync_pull_events
aequora_sync_conflicts_total
aequora_sync_rejections_total
aequora_sync_retries_total
```

---

# 24. Operation Outcome Metrics

Outcome classes:

```text
Accepted
Duplicate
RejectedValidation
RejectedAuthorization
Conflict
RetryableFailure
InternalFailure
```

Keep these stable and bounded.

---

# 25. Journal Metrics

```text
aequora_journal_append_total
aequora_journal_append_duration_seconds
aequora_journal_scan_duration_seconds
aequora_journal_events_returned
aequora_journal_retention_floor
```

Do not expose raw global sequence as a metric label.

---

# 26. Ledger Metrics

```text
aequora_ledger_lookup_duration_seconds
aequora_ledger_duplicate_total
aequora_ledger_digest_mismatch_total
```

A digest mismatch for reused `OperationId` deserves a security/diagnostic event.

---

# 27. PostgreSQL Metrics

```text
aequora_db_pool_connections
aequora_db_pool_wait_seconds
aequora_db_transaction_duration_seconds
aequora_db_statement_duration_seconds
aequora_db_deadlock_retries_total
aequora_db_serialization_retries_total
aequora_db_timeout_total
```

---

# 28. Timeline Metrics

```text
aequora_timeline_lock_wait_seconds
aequora_timeline_events_reserved
aequora_timeline_commit_duration_seconds
```

These are particularly important for detecting hot tenants.

---

# 29. Client Metrics

On clients, telemetry must be more privacy/resource conservative.

Useful local counters:

```text
sync attempts
sync successes
pending count
conflict count
bootstrap attempts
local DB errors
```

Many should remain local diagnostics unless telemetry is explicitly enabled.

---

# 30. Outbox Metrics

```text
pending operations
oldest pending age
in-flight operations
retry count distribution
```

For fleet telemetry, aggregate without exposing operation identities.

---

# 31. Bootstrap Metrics

```text
bootstrap_started_total
bootstrap_completed_total
bootstrap_failed_total
bootstrap_duration_seconds
bootstrap_bytes
bootstrap_resume_total
```

---

# 32. Snapshot Metrics

```text
snapshot_create_duration_seconds
snapshot_size_bytes
snapshot_chunks
snapshot_verify_failures_total
snapshot_download_duration_seconds
```

---

# 33. Job Metrics

```text
jobs_pending
jobs_running
jobs_retrying
jobs_dead_letter
job_duration_seconds
job_attempts
```

Bound labels to `JobKind` and outcome class.

---

# 34. Consumer Metrics

```text
consumer_lag_events
consumer_lag_seconds_advisory
consumer_failures_total
consumer_replays_total
consumer_quarantine_total
```

Sequence/event lag is stronger than timestamp lag.

---

# 35. Authority Metrics

```text
authority_epoch
authority_role
authority_checkpoint_age
authority_continuity_errors_total
```

Epoch can be a gauge/value, not a high-cardinality label.

---

# 36. Runtime Metrics

```text
process_cpu
process_memory
thread/task counts
Tokio queue/runtime metrics where available
Rayon work backlog
open file descriptors
```

Use platform/runtime support carefully.

---

# 37. Storage Metrics

Client:

```text
local DB size
critical free space
outbox bytes
snapshot staging bytes
blob cache bytes
```

Server:

```text
journal growth
ledger growth
snapshot storage
```

---

# 38. Metric Types

Use correctly:

```text
Counter   monotonically accumulated events
Gauge     current state
Histogram distribution
```

Avoid modeling everything as a gauge.

---

# 39. Histograms

Histograms are preferred for:

```text
latency
payload size
batch size
queue wait
```

Bucket design should match real workload ranges.

---

# 40. Percentiles

Compute p50/p95/p99 from histograms in monitoring backend.

Do not log every latency just to calculate percentiles.

---

# 41. Structured Logging

Every log event should have:

```text
level
event name
message
component
build ID
```

and contextual fields when relevant.

---

# 42. Event Names

Prefer stable semantic names:

```text
sync.exchange.completed
operation.rejected
authority.epoch.changed
snapshot.verify.failed
config.reload.rejected
```

rather than parsing prose messages.

---

# 43. Log Levels

Recommended semantics:

```text
ERROR — operation/component failed and requires attention
WARN  — abnormal/recoverable condition
INFO  — important lifecycle/operational event
DEBUG — detailed troubleshooting
TRACE — very verbose internal flow
```

---

# 44. Do Not Log Routine Success Excessively

At high throughput, logging every accepted operation at INFO can be expensive.

Use:

```text
metrics
sampling
DEBUG
```

for routine success.

---

# 45. Error Logs

An ERROR should include:

```text
stable ErrorCode
component
retry class
safe context
trace/request ID
```

---

# 46. Error Chain

Internal logs may preserve Rust source error chains where safe.

External/client errors remain sanitized.

---

# 47. Panic Handling

Production code should minimize panics.

If a panic occurs, capture:

```text
build ID
component
thread/task context
safe backtrace if enabled
```

without secrets.

---

# 48. Secret Redaction

Never log:

```text
password
access token
refresh token
private key
database credential
session cookie
```

---

# 49. Payload Redaction

Operation payloads are not logged by default.

If debugging requires payload visibility:

```text
explicit development/debug mode
registry field classification
redaction
bounded sampling
```

---

# 50. PII Policy

Telemetry field classification should align with Parts 14 and 29.

Possible:

```text
SafeOperational
Identifier
Sensitive
PII
Secret
Financial
```

---

# 51. Registry-Driven Redaction

Stable field registry metadata can tell diagnostic renderers whether a field may be:

```text
shown
hashed
redacted
omitted
```

---

# 52. Hashing Is Not Always Anonymization

Do not assume hashing an email/user ID removes privacy risk.

Use only when policy permits.

---

# 53. Request ID

Every HTTP request receives:

```text
RequestId
```

used for transport diagnostics.

---

# 54. Correlation ID

`CorrelationId` connects a broader workflow.

It is distinct from `RequestId`.

---

# 55. Operation ID

`OperationId` is durable idempotency identity.

Never substitute trace/span ID for it.

---

# 56. Trace ID

`TraceId` is observability identity only.

Losing a trace does not affect operation correctness.

---

# 57. Identity Separation

```text
RequestId    -> one transport request
TraceId      -> one telemetry trace
CorrelationId-> workflow grouping
OperationId  -> durable semantic operation
```

Keep them separate.

---

# 58. Trace Root

For a sync exchange:

```text
HTTP exchange span
```

is a natural root.

Child spans may include:

```text
decode
authenticate
authorize
plan
operation execution
DB transaction
journal append
response encode
```

---

# 59. Example Trace

```text
sync.exchange
├── protocol.decode
├── auth.authenticate
├── compatibility.negotiate
├── operation.process
│   ├── authz.check
│   ├── domain.validate
│   └── authority.tx
│       ├── ledger.lookup
│       ├── domain.apply
│       ├── journal.append
│       └── ledger.commit
└── protocol.encode
```

---

# 60. Do Not Trace Every Row

Tracing must remain semantically useful.

Avoid spans for every trivial internal function or DB row.

---

# 61. Span Attributes

Good bounded attributes:

```text
operation.kind
entity.type
result.class
adapter.id
protocol.version
```

Potentially sensitive IDs require policy.

---

# 62. DB Tracing

Trace:

```text
transaction
logical query class
wait
retry
```

Do not export raw SQL with sensitive bound values.

---

# 63. Async Context Propagation

Tokio tasks spawned as part of request processing should inherit or explicitly propagate trace context.

Detached durable jobs get new execution traces linked to original causation/correlation metadata.

---

# 64. Job Trace Linking

A durable job may run hours later.

Do not require one trace to stay open.

Instead:

```text
original trace
   |
   +--> JobId / causation
          |
          v
new worker trace
```

using trace links where supported.

---

# 65. External Provider Tracing

For email/webhook/payment integrations:

```text
trace outbound attempt
provider class
latency
result
```

Do not leak provider secrets or full sensitive payloads.

---

# 66. Client↔Server Trace Propagation

Optional standard trace context can cross HTTP.

The server must treat incoming trace headers as untrusted telemetry hints.

---

# 67. Trace Sampling

Full tracing for every request may be too expensive.

Use sampling.

---

# 68. Sampling Classes

Possible:

```text
AlwaysSampleCritical
SampleErrors
SampleSlow
ProbabilisticNormal
NeverSensitive
```

---

# 69. Tail Sampling

Backend/collector tail sampling can retain:

```text
errors
slow requests
rare conflicts
```

while dropping routine traces.

---

# 70. Head Sampling

Useful for low-overhead client/server decisions before work starts.

---

# 71. Security Event Sampling

Do not probabilistically discard important security signals that policy requires.

Durable security/audit events remain separate.

---

# 72. Client Telemetry

Client telemetry must be:

```text
bounded
resource-aware
privacy-aware
optional according to product/privacy policy
```

Core Aequora cannot require cloud telemetry.

---

# 73. Local Diagnostic Ring

Each client can maintain a bounded diagnostic ring:

```text
recent sync state transitions
error codes
bootstrap progress
adapter errors
config/build metadata
```

without storing payload content.

---

# 74. Diagnostic Ring Durability

Depending product:

```text
memory-only
or
bounded local persisted ring
```

Persistence must not threaten critical outbox storage.

---

# 75. Mobile Constraints

Mobile telemetry must account for:

```text
battery
metered network
background execution limits
storage
```

Batch export when appropriate.

---

# 76. Desktop Telemetry

Desktop agent can maintain richer local diagnostics while respecting privacy.

---

# 77. Offline Telemetry

Offline clients should not accumulate unbounded telemetry.

Use:

```text
bounded ring
coalescing
expiry
```

---

# 78. Telemetry Upload Priority

User data synchronization is more important than optional telemetry upload.

Scheduler should classify telemetry as low-priority background work.

---

# 79. Server Export Architecture

Concept:

```text
Aequora Process
  |
  +--> metrics exporter
  +--> trace exporter
  +--> structured log sink
```

Exporters use bounded buffers and independent failure handling.

---

# 80. OpenTelemetry Boundary

OpenTelemetry-compatible export is useful for vendor neutrality.

Aequora's internal semantic event model should not depend deeply on one exporter implementation.

---

# 81. Prometheus-Style Metrics

Pull-based metrics endpoints are useful for server deployments.

Expose only operational metrics—not secrets or private business data.

---

# 82. Metrics Endpoint Security

Depending deployment:

```text
private network
authenticated scrape
sidecar collector
```

Do not expose unrestricted diagnostics publicly.

---

# 83. Health vs Metrics

Health endpoints answer availability/readiness.

Metrics answer behavior/capacity.

Do not overload `/health` with expensive metrics.

---

# 84. SLI

A Service Level Indicator is a measured signal.

Important Aequora SLIs:

```text
sync request success
authoritative operation success
accepted-write latency
bootstrap success
bootstrap latency
availability
```

---

# 85. SLO

A Service Level Objective defines a target over an SLI.

Example:

```text
99.9% of valid sync exchanges succeed over 30 days
```

Exact targets are product/deployment decisions.

---

# 86. Do Not Count Client Errors Against Server Availability Blindly

Separate:

```text
valid server-side failures
client validation failures
auth failures
business rejections
```

A rejected invalid operation is not server downtime.

---

# 87. Availability SLI

Concept:

```text
eligible successful requests
/
eligible requests
```

Eligibility excludes known client/business rejection classes according to policy.

---

# 88. Write Latency SLI

Measure:

```text
request accepted
↓
authoritative Tx B committed
```

separately from:

```text
client received response
```

when useful.

---

# 89. End-to-End Sync Latency

Can measure:

```text
local operation durable
↓
server authoritative
↓
client reconciled
```

but client offline time must be interpreted separately.

---

# 90. Offline Delay Is Not Server Latency

A client that remains offline for 12 hours should not make server processing latency appear to be 12 hours.

Use separate metrics:

```text
local pending age
network unavailable duration
server processing latency
```

---

# 91. Bootstrap SLO

Possible:

```text
99% of eligible bootstrap attempts complete within X
```

segmented by snapshot size class.

---

# 92. Conflict Rate Is Not Availability

Conflict rate is a product/domain health signal.

A conflict can be a correct outcome.

---

# 93. Error Budgets

For SLO target `99.9%`, remaining allowed failure is the error budget.

Use error-budget burn to drive alerting.

---

# 94. Burn-Rate Alerts

Prefer:

```text
fast burn
slow burn
```

alerts over raw one-minute failure spikes.

---

# 95. Alert Philosophy

Alert when a human may need to act.

Do not alert on every anomalous log line.

---

# 96. Alert Classes

```text
Page
Ticket
Warning/Dashboard
Security Response
```

---

# 97. Page Alerts

Examples:

```text
authority unavailable
journal commit failing
severe DB saturation
split-brain/fork suspicion
mass authentication failure caused by service issue
```

---

# 98. Ticket Alerts

Examples:

```text
backup age
consumer lag
disk growth
deprecated clients
increasing dead letters
```

---

# 99. Dashboard Warnings

Examples:

```text
conflict trend
batch-size trend
snapshot growth
feature rollout behavior
```

---

# 100. Security Alerts

Examples:

```text
OperationId digest mismatch
cross-tenant access attempts
admin auth anomalies
signature verification failure
authority fork evidence
```

---

# 101. Alert Deduplication

Alerts should have stable fingerprints based on:

```text
alert rule
deployment
component
bounded dimension
```

not request IDs.

---

# 102. Alert Grouping

Group related failures to avoid storms.

Example:

```text
DB outage
```

should not create thousands of separate pages for every API node.

---

# 103. Maintenance Silence

Planned maintenance can suppress expected operational alerts.

Never suppress durable security/audit recording.

---

# 104. Alert Runbooks

Every page-level alert should link to a runbook:

```text
symptoms
verification commands
safe mitigations
escalation
recovery verification
```

---

# 105. Example: DB Saturation Runbook

```text
check pool wait
check DB CPU/IO
check timeline lock wait
check long transactions
check worker/bulk traffic
shed optional work
scale/tune based on evidence
```

---

# 106. Example: Authority Epoch Mismatch

```text
stop unsafe writes if necessary
inspect authority status
compare epoch/checkpoint
check recent failover/restore
do not manually edit epoch
follow authority recovery plan
```

---

# 107. Dashboards

Recommended dashboard families:

```text
Executive Service Health
Sync Pipeline
PostgreSQL / Authority
Client Fleet
Bootstrap / Snapshot
Jobs / Side Effects
Consumers
Security
Release / Compatibility
```

---

# 108. Service Health Dashboard

Show:

```text
SLO
error budget
request rate
p50/p95/p99 latency
availability
current release
active incidents
```

---

# 109. Sync Pipeline Dashboard

Show:

```text
exchange rate
push operations
pull events
accepted/rejected/conflict
batch distributions
latency breakdown
```

---

# 110. PostgreSQL Dashboard

Show:

```text
pool utilization
pool wait
transaction latency
deadlocks
serialization retries
timeline lock wait
journal append
DB CPU/IO
```

---

# 111. Client Fleet Dashboard

Only if product telemetry policy allows.

Show aggregates:

```text
client versions
protocol compatibility
sync success
offline distribution
bootstrap failures
storage pressure
```

---

# 112. Release Dashboard

During rollout:

```text
version distribution
error rate by version
latency by version
compatibility errors
crash rate
```

---

# 113. Feature Flag Observability

For safe rollout flags:

```text
flag generation
variant
bounded cohort
result metrics
```

Avoid business/security semantic experiments.

---

# 114. Deployment Dimensions

Useful bounded telemetry dimensions:

```text
environment
region
release
adapter
operation kind
route
result class
```

---

# 115. Release Dimension Cardinality

Keep only a bounded number of active release versions in metrics.

Historical long-term analysis can use logs/data warehouse rather than permanent metric labels.

---

# 116. Log Retention

Retention differs by class:

```text
application logs
security logs
audit
diagnostic bundles
```

Do not apply one retention policy to everything.

---

# 117. Trace Retention

Traces are typically shorter-lived than durable audit.

Retention depends on cost/security requirements.

---

# 118. Metrics Retention

Long-term aggregates may be retained longer than raw traces.

---

# 119. Governance

Telemetry data is itself governed data.

Define:

```text
collection purpose
retention
access
residency
erasure behavior where applicable
```

---

# 120. Multi-Region Telemetry

Regional collectors can aggregate locally and forward summaries/traces centrally.

Respect residency requirements.

---

# 121. Air-Gapped Telemetry

Air-gapped deployment supports:

```text
local Prometheus-compatible metrics
local log aggregation
local trace collector
```

No public SaaS dependency.

---

# 122. Telemetry Backend Independence

Aequora should work with:

```text
open-source observability stack
cloud vendor
enterprise vendor
local files
```

through adapters/exporters.

---

# 123. Backpressure

If collector is slow:

```text
bounded queue
batch
drop low-priority data
increment telemetry_dropped counter
```

Never block authority transactions indefinitely.

---

# 124. Telemetry Self-Metrics

Observe the observer:

```text
telemetry_queue_depth
telemetry_dropped_total
telemetry_export_failures_total
telemetry_export_latency
```

---

# 125. Recursive Failure

Telemetry failures must not recursively generate unlimited telemetry.

Use suppression/rate limiting.

---

# 126. Log Rate Limiting

Repeated identical errors can be:

```text
sampled
coalesced
rate limited
```

while maintaining aggregate counters.

---

# 127. First/Last/Suppressed Pattern

Useful:

```text
first occurrence logged
counter increments
periodic summary of suppressed repeats
last/recovery logged
```

---

# 128. Recovery Events

Log meaningful recovery:

```text
database connectivity restored
authority healthy
consumer caught up
```

so incidents have clear boundaries.

---

# 129. State Transition Logging

Important state machines can emit transition events:

```text
client sync state
bootstrap
job
repair
authority
consumer
```

Avoid logging unchanged polling states.

---

# 130. State Transition Schema

Concept:

```rust
struct StateTransitionEvent<S> {
    from: S,
    to: S,
    reason: ReasonCode,
}
```

---

# 131. Diagnostic Event Model

Concept:

```rust
pub struct DiagnosticEvent {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub component: ComponentId,
    pub context: SafeContext,
}
```

Stable codes make support tooling easier.

---

# 132. Diagnostic Codes

Examples:

```text
AEQ-DIAG-DB-001
AEQ-DIAG-SYNC-004
AEQ-DIAG-AUTHORITY-002
```

---

# 133. Event Registry

Durable/public diagnostic event IDs should follow Part 29 registry governance.

---

# 134. Operation Explainability

Given `OperationId`, tooling should correlate:

```text
local outbox state
request/correlation IDs
server ledger outcome
journal events
conflict
job causation
```

without relying on one trace being retained forever.

---

# 135. Why Traces Cannot Be the Source of Truth

Traces may be:

```text
sampled
dropped
expired
backend unavailable
```

Therefore durable operation/journal/ledger metadata remains the forensic source of truth.

---

# 136. Correlation Index

Operational tooling may index:

```text
OperationId -> recent trace IDs
```

as an optimization, never a correctness dependency.

---

# 137. Incident Bundles

Part 25 bundles should include:

```text
relevant metric snapshots
safe structured logs
selected trace fragments
build ID
config digest
registry digest
authority metadata
diagnostic ring
```

---

# 138. Incident Time Window

Collect bounded windows around:

```text
event time
OperationId
JobId
ConflictId
```

to minimize unrelated data.

---

# 139. Clock Semantics

Telemetry timestamps use wall clock for human correlation.

They are not authoritative ordering.

---

# 140. Clock Skew

Trace/log systems must tolerate clock skew.

Aequora causal/journal ordering still comes from:

```text
timeline sequence
operation dependencies
HLC where applicable
```

---

# 141. HLC in Telemetry

HLC may be included for diagnostics but does not replace trace timestamps or journal sequence.

---

# 142. Monotonic Timers

Latency measurement should use monotonic clocks, not wall-clock subtraction.

---

# 143. Startup Telemetry

On startup emit one structured event containing safe metadata:

```text
release
build
environment
deployment
protocol range
registry digest
config generation/digest
adapter IDs
```

---

# 144. Shutdown Telemetry

Record:

```text
graceful shutdown start
drain result
shutdown complete
```

when possible.

Abrupt crash obviously may not emit completion.

---

# 145. Configuration Reload

Emit:

```text
old generation
new generation
changed safe keys/classes
result
```

Never secret values.

---

# 146. Migration Telemetry

Record:

```text
migration ID
from/to version
duration
result
```

with authoritative migration ledger remaining source of truth.

---

# 147. Snapshot Telemetry

Trace:

```text
snapshot plan
DB read
chunk encode/compress/hash
upload
manifest publish
```

without one giant unbounded span payload.

---

# 148. Anti-Entropy Telemetry

Metrics:

```text
integrity checks
partitions compared
divergence detected
repair plans
repair success
```

Divergence deserves strong diagnostics.

---

# 149. Conflict Telemetry

Measure:

```text
conflicts by operation/entity type
resolution latency
resolution outcome
```

Avoid identifying user/entity labels.

---

# 150. Scheduler Telemetry

Useful:

```text
work selected
work deferred by network/power
batch size
backoff
data budget
```

Client export remains privacy/resource constrained.

---

# 151. Admission Telemetry

```text
admitted
rejected global
rejected tenant
rejected resource
queue wait
```

This is essential for overload diagnosis.

---

# 152. Brownout Telemetry

When optional work is shed, expose:

```text
brownout state
shed work classes
duration
```

---

# 153. Security Telemetry

Security event model should include:

```text
event code
actor class
tenant if policy allows
source class
decision
reason
```

not raw credentials.

---

# 154. Authentication Metrics

```text
auth_success_total
auth_failure_total
token_expired_total
device_revoked_total
```

Use bounded reason classes.

---

# 155. Authorization Metrics

```text
authz_denied_total
```

segmented by bounded resource/action class.

A sudden spike may indicate attack or release regression.

---

# 156. Abuse Metrics

```text
rate_limit_rejected
oversized_request_rejected
decode_failure
dependency_graph_limit
compression_ratio_rejected
```

---

# 157. Privacy-Safe Defaults

Production defaults:

```text
no payload logging
no raw tokens
no secret values
no arbitrary SQL values
no unrestricted user identifiers in metrics
bounded trace sampling
```

---

# 158. Debug Mode

Debug mode may increase telemetry detail but must still redact secrets.

Production activation should be:

```text
time bounded
audited where applicable
targeted
```

---

# 159. Per-Tenant Debugging

For difficult incidents, a temporary diagnostic filter may target one tenant.

It must have:

```text
authorization
expiry
privacy controls
audit
```

---

# 160. Per-Operation Debugging

Prefer targeting:

```text
OperationId
CorrelationId
```

over globally enabling verbose logging.

---

# 161. Dynamic Log Level

Part 43 runtime policy may permit safe log-level reload.

Changing log level must not change business behavior.

---

# 162. Observability Configuration

Example:

```ron
observability: (
    log_level: "info",

    metrics: (
        enabled: true,
        endpoint: "127.0.0.1:9090",
    ),

    tracing: (
        enabled: true,
        sample_ratio: 0.05,
    ),
)
```

Secrets for remote exporters remain `SecretRef`s.

---

# 163. Environment Defaults

Development:

```text
human logs
DEBUG optional
local metrics
high trace sampling
```

Production:

```text
structured logs
INFO
bounded sampling
private metrics
```

---

# 164. Log Formats

Support:

```text
human
JSON
```

RON is useful for config/artifacts, but JSON is often the correct boundary for log collectors.

---

# 165. stdout/stderr

Container/server recommendation:

```text
structured logs -> stdout/stderr
```

and let platform collectors route them.

Native/systemd can use journald or file sinks.

---

# 166. File Logging

If supported:

```text
rotation
size limits
retention
permissions
```

are mandatory.

Never let logs fill the disk containing critical local data.

---

# 167. Client Log Storage

Client logs should have strict quotas and lower priority than:

```text
outbox
database
snapshot staging required for recovery
```

---

# 168. Telemetry and Disk Pressure

Under disk pressure:

```text
delete/rotate optional logs first
```

never durable unsynced intent.

---

# 169. SLO Ownership

Each SLO needs:

```text
owner
definition
measurement source
window
target
exclusions
runbook
```

---

# 170. Example Sync Availability SLO

Concept:

```text
SLI:
eligible successful / eligible sync exchanges

Window:
30 days

Target:
99.9%
```

The exact number is a product/business decision.

---

# 171. Example Authority Commit SLO

Measure successful valid authoritative operations that complete within a defined latency threshold.

Keep separate from conflicts/business rejections.

---

# 172. Client Fleet SLO

Possible:

```text
percentage of active compatible clients completing at least one successful sync over expected connectivity window
```

but interpret offline populations carefully.

---

# 173. SLO Segmentation

Segment only by bounded meaningful dimensions:

```text
region
release
platform
adapter
operation class
```

---

# 174. Alert on Symptoms First

Prefer alerting:

```text
users cannot sync
```

over:

```text
CPU is 82%
```

Resource alerts still matter as predictive/ticket signals.

---

# 175. Saturation Signals

Track:

```text
DB pool
CPU
memory
disk
network
worker queues
timeline locks
```

---

# 176. RED Method

For request-oriented components:

```text
Rate
Errors
Duration
```

---

# 177. USE Method

For resources:

```text
Utilization
Saturation
Errors
```

---

# 178. Golden Signals

Useful overall:

```text
latency
traffic
errors
saturation
```

---

# 179. Release Regression Detection

Compare new release against baseline:

```text
error rate
latency
conflicts
DB retries
memory
CPU
```

during canary rollout.

---

# 180. Compatibility Telemetry

Track:

```text
protocol negotiation failures
minimum-client rejections
deprecated schema usage
rebootstrap-required responses
```

This informs deprecation decisions.

---

# 181. Registry Telemetry

Startup can report registry generation/digest.

Do not use registry digest as a metric label if it creates excessive cardinality; expose via build/info metric or diagnostics.

---

# 182. Build Info Metric

A common bounded gauge:

```text
aequora_build_info{version="...",target="..."} 1
```

Keep labels limited.

---

# 183. Fleet Version Query

For detailed version inventory, use control-plane/node registry rather than abusing time-series metrics.

---

# 184. Metrics Aggregation

Counters aggregate naturally across nodes.

Gauges such as queue size need clear interpretation:

```text
per-node
sum
max
```

---

# 185. Authority-Wide Metrics

Some metrics are DB-derived and should not be summed from every API node.

Examples:

```text
journal retention floor
authority epoch
```

Expose from a designated collector or query carefully.

---

# 186. Database Exporter

A separate PostgreSQL exporter can provide engine metrics.

Aequora-specific DB semantics remain instrumented by Aequora.

---

# 187. Telemetry Cost Budget

Observability has a resource budget:

```text
CPU
memory
network
storage
vendor cost
```

Set explicit limits.

---

# 188. Trace Payload Budget

Never attach entire operation/snapshot payloads to spans.

---

# 189. Attribute Budget

Limit:

```text
attribute count
attribute length
event count per span
```

---

# 190. Metric Series Budget

CI/load testing should detect accidental cardinality explosions.

---

# 191. Observability Benchmarking

Part 47 verification and Part 46 performance-related tooling should measure telemetry overhead at:

```text
off
normal production
debug sampling
```

---

# 192. Zero-Telemetry Mode

Air-gapped/minimal products may disable remote exporters.

Local diagnostics and durable audit still work.

---

# 193. No-Op Provider

Core supports:

```text
NoopMetrics
NoopTracer
```

without branching correctness logic.

---

# 194. Testing

Required:

```text
metric naming
label cardinality
trace propagation
redaction
sampling
export failure
bounded queues
log rate limiting
incident correlation
SLO classification
```

---

# 195. Redaction Tests

Inject sentinel:

```text
SECRET_DO_NOT_LEAK_123
```

through:

```text
config
auth token
operation payload
DB error
provider error
```

and verify it does not appear in telemetry.

---

# 196. Cardinality Tests

Generate thousands of:

```text
OperationId
DeviceId
TenantId
```

and verify metric series count remains bounded.

---

# 197. Exporter Failure Test

Make collector unavailable.

Expected:

```text
Aequora continues
bounded queue fills
low-priority telemetry drops
self-metric increments
```

---

# 198. Slow Exporter Test

Ensure exporter backpressure does not hold:

```text
DB transaction
authority lock
request permit
```

indefinitely.

---

# 199. Trace Sampling Test

Errors/slow requests should be retained according to policy while normal requests follow configured sampling.

---

# 200. Clock Test

Wall-clock jumps must not produce negative latency because durations use monotonic time.

---

# 201. Multi-Process Client Test

Desktop GUI/agent telemetry must not double-count logical operations merely because multiple processes observe the same store.

Define ownership of emitted metrics.

---

# 202. Observability Conformance

A future conformance profile can verify that official components expose required semantic telemetry without leaking prohibited data.

---

# 203. Operational CLI

Useful commands:

```text
aequora diagnostics summary
aequora diagnostics operation <id>
aequora diagnostics metrics
aequora diagnostics trace <id>
aequora incident collect
```

---

# 204. Metrics Snapshot

`aequora diagnostics metrics` can show a bounded local snapshot without requiring a monitoring backend.

---

# 205. Trace Lookup

Trace lookup is best-effort.

If trace expired, CLI falls back to durable operation/journal/ledger diagnostics.

---

# 206. Alert Metadata

A stable alert definition can include:

```rust
struct AlertDefinition {
    id: AlertId,
    severity: AlertSeverity,
    owner: TeamId,
    runbook: RunbookId,
}
```

---

# 207. Stable Alert IDs

Examples:

```text
AEQ-ALERT-AUTHORITY-001
AEQ-ALERT-DB-002
AEQ-ALERT-SYNC-003
```

---

# 208. Observability Invariants

## AEQ-INV-OBS001

```text
Loss, delay, or failure of metrics, logs, traces, or external telemetry collectors cannot alter synchronization correctness or authoritative commit semantics.
```

## AEQ-INV-OBS002

```text
Unbounded identifiers such as OperationId, EntityId, DeviceId, RequestId, and arbitrary TenantId values are not used as ordinary metric labels.
```

## AEQ-INV-OBS003

```text
Secrets and prohibited sensitive payload fields never appear in production metrics, logs, traces, diagnostic bundles, or exporter errors.
```

## AEQ-INV-OBS004

```text
Durable audit, operation ledger, journal, and authoritative metadata remain the forensic source of truth; sampled telemetry is never treated as authoritative history.
```

## AEQ-INV-OBS005

```text
Telemetry buffering and export are bounded and cannot exhaust resources needed for outbox durability, authority transactions, or core synchronization.
```

## AEQ-INV-OBS006

```text
RequestId, TraceId, CorrelationId, and OperationId remain distinct identities with distinct semantics.
```

## AEQ-INV-OBS007

```text
Latency measurements use monotonic timing; wall-clock timestamps are advisory correlation metadata and never authoritative ordering.
```

## AEQ-INV-OBS008

```text
SLO calculations distinguish infrastructure/server failures from expected validation, authorization, conflict, and business-domain outcomes according to explicit classification.
```

## AEQ-INV-OBS009

```text
Client telemetry remains bounded, resource-aware, privacy-governed, and lower priority than durable user intent synchronization.
```

## AEQ-INV-OBS010

```text
Every page-level production alert has a stable identity, an actionable condition, an owner, and a recovery/diagnostic runbook.
```

---

# 209. Recommended v1 Observability Surface

Server:

```text
structured tracing logs
Prometheus-compatible metrics
optional OpenTelemetry traces
health/readiness
diagnostic CLI
```

Client:

```text
bounded local diagnostic ring
sync status
pending/conflict/bootstrap diagnostics
optional privacy-governed telemetry export
```

Durable:

```text
operation ledger
journal
business/admin audit
job records
authority metadata
```

---

# 210. Recommended Initial Metrics

Start small:

```text
HTTP rate/error/latency
sync rate/error/latency
accepted/rejected/conflict operations
DB pool wait
DB transaction latency
timeline lock wait
journal append latency
job backlog
bootstrap success/latency
telemetry drops
build/version info
```

Add metrics only when there is an operational question they answer.

---

# 211. Recommended Initial Dashboards

```text
1. Service Health
2. Sync Pipeline
3. PostgreSQL / Authority
4. Jobs / Background Work
5. Release / Compatibility
```

Add fleet/security/snapshot dashboards as deployment maturity grows.

---

# 212. Recommended Initial Alerts

Page:

```text
authority commit failure
sustained sync availability burn
database unavailable/saturated
authority continuity/fork evidence
```

Ticket:

```text
backup stale
consumer/job backlog
disk/storage growth
deprecated client population
```

---

# 213. Workspace Placement

Possible:

```text
crates/operations/
├── aequora-observability/
└── aequora-diagnostics/

apps/
└── aequora/

deploy/
└── observability/
    ├── dashboards/
    ├── alerts/
    └── collectors/
```

Vendor-specific dashboards/exporters remain outside core crates.

---

# 214. Final Architecture

```text
                     Aequora Components
                            |
       +--------------------+--------------------+
       |                    |                    |
       v                    v                    v
    Metrics           Structured Logs       Traces
       |                    |                    |
       +--------------------+--------------------+
                            |
                            v
                  Bounded Export Layer
                            |
              +-------------+-------------+
              |             |             |
              v             v             v
          Metrics DB      Log Store    Trace Store
              |             |             |
              +-------------+-------------+
                            |
                            v
                  Dashboards / Alerts
```

Separate durable truth:

```text
Authoritative PostgreSQL
├── operation ledger
├── journal
├── audit
├── jobs
└── authority metadata
```

and client forensic state:

```text
SQLite / Stoolap
├── outbox
├── cursor
├── conflicts
└── bounded diagnostic ring
```

---

# 215. Final Recommendation

Aequora should instrument the semantic boundaries that matter:

```text
local durability
transport
authentication
authorization
planning
authority transaction
journal
reconciliation
bootstrap
jobs
authority continuity
```

rather than producing enormous quantities of low-value logs.

The production hierarchy should be:

```text
metrics tell us something is wrong
↓
traces/logs tell us where
↓
durable Aequora metadata tells us exactly what happened
↓
incident/replay tooling proves and reproduces it
```

> **Telemetry should make Aequora observable when everything works and explainable when it fails, while remaining bounded, privacy-safe, vendor-neutral, and completely unnecessary for correctness.**

---

## Next

**Part 47 — Benchmarking, Performance Regression, Capacity Planning, Workload Modeling, Scalability Testing, and Production Sizing Architecture**
