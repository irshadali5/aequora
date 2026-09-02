# Aequora Sync — Part 41

# CLI, Developer Toolchain, Inspection, Verification, Migration, and Operational Developer Experience Architecture

## 1. Purpose

Part 41 turns Aequora's architecture into an operable developer system.

The primary executable is:

```text
aequora
```

with supporting crates such as:

```text
aequora-cli
aequora-cli-core
aequora-devtools
aequora-inspect
```

The central rule is:

> **The CLI is a typed client of stable Aequora application/control APIs, not a privileged shortcut around invariants.**

A developer tool must never gain correctness by directly editing internal tables.

---

## 2. Goals

The toolchain should make these workflows first-class:

```text
initialize integration
validate configuration
inspect local/server state
run sync manually
inspect operations/conflicts
manage schemas and registries
run migrations
verify adapters
run conformance
diagnose incidents
export safe diagnostics
inspect compatibility
manage snapshots
inspect jobs/consumers
perform controlled admin actions
generate project scaffolding
```

The CLI should work for:

```text
local development
CI
production operations
support/forensics
adapter development
application integration
```

---

## 3. Non-Goals

The CLI is not:

```text
a generic SQL shell
a replacement for psql
a way to bypass authorization
a mutable internal-table editor
a hidden superuser backdoor
a second implementation of sync semantics
```

---

## 4. Workspace Structure

```text
crates/tooling/
├── aequora-cli-core/
├── aequora-cli/
├── aequora-devtools/
├── aequora-inspect/
├── aequora-registry-cli/
├── aequora-conformance/
└── aequora-testkit/

apps/
└── aequora/
    └── src/main.rs
```

`main.rs` should be a composition root, not a large command implementation.

---

## 5. Dependency Direction

```text
aequora binary
      |
      v
aequora-cli
      |
      v
aequora-cli-core
      |
      +--> public SDK
      +--> control-plane client
      +--> registry tooling
      +--> conformance/testkit
```

Avoid:

```text
CLI -> private PostgreSQL tables
CLI -> Stoolap internals
CLI -> internal server structs
```

unless an explicitly offline repair tool is designed and guarded for that physical adapter.

---

## 6. Command Hierarchy

Recommended top-level shape:

```text
aequora
├── init
├── doctor
├── config
├── client
├── sync
├── operation
├── conflict
├── scope
├── registry
├── schema
├── migrate
├── adapter
├── conform
├── snapshot
├── integrity
├── repair
├── job
├── consumer
├── authority
├── diagnostics
├── incident
├── admin
├── dev
└── version
```

Keep top-level nouns stable.

---

## 7. Rust CLI Stack

A practical pure-Rust implementation can use:

```text
clap
serde
RON
postcard
tokio
tracing
tracing-subscriber
indicatif or equivalent optional progress UI
comfy-table or equivalent optional terminal table layer
```

Dependencies remain subject to Aequora's license and supply-chain policy.

---

## 8. Typed Command Model

Parse into typed commands:

```rust
enum Command {
    Doctor(DoctorArgs),
    Sync(SyncCommand),
    Registry(RegistryCommand),
    Adapter(AdapterCommand),
    Diagnostics(DiagnosticsCommand),
}
```

Do not pass raw string maps throughout the program.

---

## 9. Strong CLI Types

Parse values immediately into:

```text
TenantId
DeviceId
OperationId
ConflictId
ScopeId
AuthorityId
Epoch
Cursor
Duration
ByteSize
```

Invalid identifiers fail before reaching services.

---

## 10. Human and Machine Output

Every important read-only command should support both:

```text
human output
machine output
```

For example:

```text
--output human
--output json
--output ron
```

Postcard may be supported for artifacts, but binary stdout should be explicit.

---

## 11. Stable Automation Output

Human output may evolve cosmetically.

Machine output needs:

```text
schema version
stable field names
stable error codes
```

CI scripts should never scrape decorative terminal text.

---

## 12. Exit Codes

Define stable categories:

```text
0 success
2 invalid CLI usage/config
3 authentication/authorization
4 compatibility failure
5 validation/conformance failure
6 temporary dependency unavailable
7 operation failed
8 recovery/manual intervention required
```

Exact numbers should be centrally registered.

---

## 13. Error Envelope

Machine output should contain:

```rust
struct CliErrorEnvelope {
    schema_version: u16,
    code: ErrorCode,
    retry: RetryClass,
    message: String,
    details: Option<SafeDetails>,
}
```

Never expose secrets or raw credentials.

---

## 14. `aequora init`

Purpose:

```text
create project integration skeleton
```

Possible output:

```text
aequora.ron
registry/
migrations/
docs/
example composition root
```

It should not overwrite existing files without explicit approval.

---

## 15. Templates

Initial templates:

```text
rust-client
rust-server
dioxus-client
axum-server
postgres-authority
stoolap-local
full-local-first
```

Templates should be versioned.

---

## 16. Generated vs Owned Files

Clearly mark:

```text
generated files
developer-owned files
```

Never regenerate over developer business code silently.

---

## 17. `aequora doctor`

This should become one of the most useful commands.

It checks:

```text
config parse
required secrets resolvable
local store accessibility
store schema version
registry validity
server reachability
protocol compatibility
authority health
adapter capabilities
migration state
filesystem permissions
clock sanity
disk availability
```

---

## 18. Doctor Result

Use structured checks:

```text
PASS
WARN
FAIL
SKIP
```

with stable check IDs.

Example:

```text
AEQ-DOCTOR-STORE-001
AEQ-DOCTOR-PROTOCOL-002
```

---

## 19. Doctor Must Not Mutate

Default `doctor` is read-only.

If remediation is possible:

```text
aequora doctor --fix
```

must list planned changes before applying anything destructive.

---

## 20. Configuration Commands

```text
aequora config check
aequora config explain
aequora config effective
```

`effective` shows merged non-secret configuration and provenance.

---

## 21. Secret Redaction

Always render:

```text
DATABASE_URL = <redacted>
TOKEN = <redacted>
```

Optionally show:

```text
source = environment
```

without the value.

---

## 22. Configuration Provenance

Useful:

```text
value
source
generation
```

Example:

```text
sync.batch.max_operations = 256
source = /etc/aequora/server.ron
```

---

## 23. Client Commands

Possible:

```text
aequora client status
aequora client info
aequora client pending
aequora client pause
aequora client resume
```

Mutation of runtime state should use the public client/agent API.

---

## 24. `sync status`

Display:

```text
active store
active tenant
sync status
pending operations
open conflicts
active scopes
authority/epoch
last successful exchange
backoff state
```

Do not infer correctness from timestamps alone.

---

## 25. `sync now`

```text
aequora sync now
```

requests one bounded synchronization cycle.

Useful options:

```text
--wait
--scope <id>
--timeout <duration>
```

It must not bypass scheduler safety or server admission control.

---

## 26. Operation Inspection

```text
aequora operation show <operation-id>
aequora operation list --state pending
aequora operation explain <operation-id>
```

`explain` can show:

```text
operation kind
schema version
local sequence
state
dependencies
attempt history
server outcome
causation/correlation
conflict link
```

subject to privacy policy.

---

## 27. Payload Display

Sensitive payload fields are redacted by default according to registry metadata.

Explicit reveal should require an intentional flag and authorization where applicable.

---

## 28. Operation Mutation

Avoid commands like:

```text
operation mark-accepted
operation delete
```

Those violate authority/idempotency semantics.

Permitted actions are semantic:

```text
cancel eligible unsent operation
retry eligible operation
submit domain-defined correction
```

---

## 29. Conflict Commands

```text
aequora conflict list
aequora conflict show <id>
aequora conflict explain <id>
```

Resolution should invoke a registered resolution operation, not edit conflict metadata directly.

---

## 30. Scope Commands

```text
aequora scope list
aequora scope show <id>
aequora scope request ...
aequora scope release <id>
```

The CLI uses the same scope semantics as GUI clients.

---

## 31. Registry Commands

From Part 29:

```text
aequora registry verify
aequora registry lint
aequora registry diff
aequora registry explain
aequora registry reserve
aequora registry docs
aequora registry compatibility
```

---

## 32. Registry Verification

Checks include:

```text
duplicate IDs
ID reuse
illegal status transition
missing owner
missing migration
schema regression
capability inconsistency
invalid semantic digest
```

---

## 33. Registry Diff

Classify changes:

```text
NonBreaking
Additive
Deprecated
BreakingWithMigration
SecurityRequired
```

This should be CI-friendly.

---

## 34. Registry Code Generation

```text
aequora registry generate
```

produces deterministic generated Rust/docs/fixtures.

CI verifies:

```text
generated output == checked-in output
```

where generated files are committed.

---

## 35. Schema Commands

Separate:

```text
protocol schema
domain schema
adapter physical schema
local store format
snapshot schema
```

Do not use one ambiguous "schema version".

---

## 36. Migration Commands

```text
aequora migrate status
aequora migrate plan
aequora migrate apply
aequora migrate verify
```

---

## 37. Plan Before Apply

`migrate plan` should report:

```text
current version
target version
migration IDs
checksums
estimated risk class
requires downtime?
backup recommendation
rollback properties
```

---

## 38. Migration Safety

Production `apply` should verify:

```text
migration checksum
expected source version
authority/store identity
required capabilities
exclusive/fenced maintenance if required
```

---

## 39. Local Migration

Local migration must preserve:

```text
outbox
cursor
conflicts
store identity
bootstrap state
```

No CLI shortcut may reset these unless explicitly executing a recovery plan.

---

## 40. Server Migration

Server migration must preserve:

```text
journal
operation ledger
authority epoch
timeline
retention state
audit linkage
```

---

## 41. Adapter Commands

```text
aequora adapter list
aequora adapter describe <adapter>
aequora adapter capabilities <adapter>
aequora adapter verify <adapter>
```

---

## 42. Adapter Descriptor

Display:

```text
AdapterId
adapter version
store role
engine version
capability manifest
certification tier
known limitations
```

---

## 43. Adapter Verification

Quick verification is not full certification.

It can test:

```text
connection
schema
transaction support
required indexes
critical configuration
read/write probe
```

using a disposable test namespace where needed.

---

## 44. Conformance Commands

```text
aequora conform local-store
aequora conform authority
aequora conform snapshot
aequora conform protocol
aequora conform all
```

---

## 45. Conformance Evidence

Produce:

```text
human report
machine RON/JSON
test IDs
environment fingerprint
adapter version
DB version
feature flags
seed
failure traces
```

---

## 46. CI Mode

```text
aequora conform authority --ci
```

should:

```text
disable interactive prompts
emit stable output
use deterministic paths
return meaningful exit codes
```

---

## 47. Snapshot Commands

```text
aequora snapshot list
aequora snapshot inspect <id>
aequora snapshot verify <id>
aequora snapshot create ...
aequora snapshot gc --plan
```

---

## 48. Snapshot Create

Creation should call the authoritative snapshot service.

The CLI must not improvise a database dump and label it an Aequora snapshot.

---

## 49. Snapshot Verify

Checks:

```text
manifest signature
hashes
chunk availability
schema compatibility
authority/epoch
boundary cursor
```

---

## 50. Snapshot GC

Always support dry-run/plan.

Never delete artifacts still protected by:

```text
retention
bootstrap leases
legal hold
recovery policy
```

---

## 51. Integrity Commands

```text
aequora integrity status
aequora integrity verify
aequora integrity compare
```

These invoke Part 03 anti-entropy/integrity mechanisms.

---

## 52. Repair Commands

```text
aequora repair plan
aequora repair apply <plan-id>
aequora repair status <repair-id>
```

No unplanned destructive repair.

---

## 53. Repair Plan

Contains:

```text
RepairId
affected scope/entities
divergence classification
authoritative source
expected mutations
risk
required permissions
```

---

## 54. Jobs

```text
aequora job list
aequora job show <id>
aequora job retry <id>
aequora job cancel <id>
```

Only actions permitted by job semantics are exposed.

---

## 55. Side Effects

For provider ambiguity:

```text
aequora job reconcile <id>
```

may invoke a typed reconciliation workflow.

Never blindly resend ambiguous payments/emails/webhooks.

---

## 56. Change Feed Consumers

```text
aequora consumer list
aequora consumer show <id>
aequora consumer lag <id>
aequora consumer reset --plan ...
```

---

## 57. Consumer Cursor Reset

Cursor reset is dangerous.

Require:

```text
plan
reason
authorization
audit
```

and show whether replay/rebuild is possible.

---

## 58. Authority Commands

```text
aequora authority status
aequora authority timeline
aequora authority checkpoint
```

Promotion/failover commands belong to high-assurance control-plane workflows.

---

## 59. Authority Promotion

Concept:

```text
aequora authority promote --plan
aequora authority promote --apply <plan-id>
```

Require:

```text
fencing
continuity assessment
new epoch if needed
step-up authorization
audit
```

---

## 60. Never Offer `--force` as Magic

Avoid a universal:

```text
--force
```

that disables invariants.

Use specific explicit acknowledgements:

```text
--accept-rebootstrap
--acknowledge-data-loss-risk
```

only where architecture permits the action.

---

## 61. Diagnostics

```text
aequora diagnostics summary
aequora diagnostics client
aequora diagnostics server
aequora diagnostics operation <id>
```

---

## 62. Incident Bundle

```text
aequora incident collect
aequora incident inspect <bundle>
aequora incident replay <bundle>
```

Part 25 privacy and reproducibility rules apply.

---

## 63. Incident Bundle Defaults

Default bundle should:

```text
redact secrets
minimize PII
include version/build metadata
include invariant/error IDs
include safe logs/traces
include registry/protocol fingerprints
```

---

## 64. Reproducible Replay

```text
aequora incident replay
```

uses deterministic test/replay infrastructure, never production side effects.

---

## 65. Admin Commands

`aequora admin` is a client for the Part 24 control plane.

It should not connect directly to DB tables.

---

## 66. Admin Authentication

Support:

```text
separate admin credential
mTLS
step-up MFA
short-lived token
```

depending deployment.

---

## 67. Plan/Apply Pattern

Dangerous admin operations use:

```text
plan
↓
review
↓
apply plan ID
```

The plan can expire.

---

## 68. Audit Reason

High-impact actions require:

```text
--reason "..."
```

with a registered ReasonCode where applicable.

---

## 69. Interactive Confirmation

Interactive confirmation is helpful for humans but not a security boundary.

Authorization and policy enforcement remain server-side.

---

## 70. Non-Interactive Production Actions

CI/automation can use explicit:

```text
--apply <plan-id>
```

instead of answering prompts.

---

## 71. Development Commands

```text
aequora dev server
aequora dev client
aequora dev seed
aequora dev reset
aequora dev fixture
```

These are clearly development-only.

---

## 72. Dev Reset

`dev reset` must refuse production profiles by default.

Use environment/store markers to prevent accidental production reset.

---

## 73. Seed Data

Seed through:

```text
registered domain operations
or
defined bootstrap/import mechanism
```

not ad-hoc SQL when semantic history matters.

---

## 74. Fixtures

Fixtures should be deterministic and versioned.

Suggested:

```text
fixtures/
├── protocol/
├── operations/
├── snapshots/
├── conflicts/
└── failures/
```

---

## 75. Golden Protocol Fixtures

CLI:

```text
aequora dev fixture verify
```

can validate Postcard golden compatibility.

---

## 76. Protocol Inspection

Useful developer command:

```text
aequora dev protocol decode <file>
aequora dev protocol encode <file>
```

Only for safe development artifacts.

---

## 77. RON as Developer Format

RON is appropriate for:

```text
config
fixtures
registry source
test scenarios
human-reviewable plans
```

Postcard remains canonical binary transport/storage where specified.

---

## 78. JSON Boundary

JSON output exists for interoperability with shell/CI/external tools.

This is an appropriate JSON use despite Aequora preferring Postcard/RON internally.

---

## 79. Shell Completion

Generate:

```text
bash
zsh
fish
PowerShell
```

completion scripts.

---

## 80. Man Pages

Generate command reference/man pages from the same command metadata to avoid documentation drift.

---

## 81. `--help`

Every command should explain:

```text
what it does
whether it mutates
required permissions
examples
```

Dangerous commands explicitly say so.

---

## 82. Version Command

```text
aequora version
```

should show:

```text
CLI version
build commit
Rust target
enabled features
protocol range
registry generation/digest
```

No secrets.

---

## 83. Compatibility Command

```text
aequora compatibility check --server ...
```

reports:

```text
protocol overlap
required capabilities
minimum client policy
operation schemas
rebootstrap requirement
```

---

## 84. Offline Inspection

For local stores, support read-only offline inspection through adapter SDK interfaces.

Example:

```text
aequora client inspect --store ./data
```

---

## 85. Read-Only by Default

Opening a store for inspection should use read-only mode where supported.

This prevents accidental migrations/mutations.

---

## 86. Store Locking

Respect Part 05/32 fencing and coordination.

CLI must not open the same embedded store for unsafe writes while the GUI/agent owns it.

---

## 87. Agent-Aware CLI

On desktop, preferred:

```text
CLI
↓
local authenticated IPC
↓
aequora-agent
```

when the agent owns the store.

---

## 88. Direct Store Mode

Direct write access is permitted only when:

```text
store is offline
ownership/fencing acquired
command explicitly requires it
```

---

## 89. Production Server Inspection

Use control/data APIs rather than direct database credentials whenever possible.

---

## 90. `psql` Still Exists

Operators can use PostgreSQL-native tools for DB administration, but Aequora's CLI should not pretend arbitrary SQL is semantically safe Aequora administration.

---

## 91. Logging

Global options:

```text
--log-level
--log-format human|json
--trace-id
```

Sensitive values are redacted.

---

## 92. Progress Bars

Use progress bars only for TTY human mode.

Machine mode emits structured progress/events or final result.

---

## 93. Color

Support:

```text
--color auto|always|never
```

Do not encode meaning by color alone.

---

## 94. Accessibility

Human output should remain understandable:

```text
without color
with screen readers
in narrow terminals
```

---

## 95. Paging

Large human output may use a pager only when interactive.

Never unexpectedly invoke a pager in CI.

---

## 96. Destructive Action Safety Classes

Classify commands:

```text
ReadOnly
LowRiskMutation
OperationalMutation
Destructive
AuthorityCritical
```

---

## 97. Policy by Safety Class

For example:

```text
ReadOnly -> no confirmation
LowRisk -> ordinary auth
Operational -> explicit apply
Destructive -> plan/apply + reason
AuthorityCritical -> plan/apply + step-up + audit/fencing
```

---

## 98. Command Metadata

Each command can declare:

```rust
struct CommandPolicy {
    safety: SafetyClass,
    required_permissions: PermissionSet,
    supports_dry_run: bool,
}
```

This enables consistent UX.

---

## 99. Dry Run

Use:

```text
--dry-run
```

only where the service can actually compute a reliable plan.

Do not fake dry-run by executing and rolling back external side effects.

---

## 100. Idempotent CLI Requests

Mutating control-plane requests should carry:

```text
AdminOperationId
```

or equivalent idempotency identity.

A CLI retry must not duplicate destructive actions.

---

## 101. Ctrl-C

Cancellation semantics:

```text
before durable submission -> abort
after durable submission -> stop waiting only
```

The CLI should report the operation/plan/job ID so the user can inspect later.

---

## 102. Network Loss

If network fails after server commit:

```text
retry same idempotency identity
```

or inspect the known operation ID.

---

## 103. Timeout

Timeout means:

```text
client stopped waiting
```

not necessarily:

```text
server did nothing
```

This distinction must be explicit.

---

## 104. CI Integration

Typical CI:

```text
aequora registry verify
aequora migrate verify
aequora conform ...
cargo test
cargo clippy
```

---

## 105. Git Hooks

Optional developer hooks may run quick checks.

Do not make local hooks the only enforcement; CI remains authoritative.

---

## 106. Generated CI Snippets

`aequora init` can generate example:

```text
GitHub Actions
GitLab CI
generic shell
```

but core tooling remains platform-neutral.

---

## 107. IDE Integration

Future IDE extensions should call:

```text
CLI machine output
or
stable developer API
```

rather than reimplement registry/migration logic.

---

## 108. Language Server Possibility

A future Aequora language/tool server could provide:

```text
RON registry validation
operation ID lookup
schema references
migration diagnostics
```

but is not required for v1.

---

## 109. Developer Explainability

Commands should answer:

```text
Why is this operation pending?
Why is this client rebootstrap-required?
Why is this adapter unsupported?
Why did this conflict occur?
Why can this snapshot not be GC'd?
```

This is a major productivity feature.

---

## 110. `explain` Pattern

Prefer consistent:

```text
aequora <resource> explain <id>
```

where useful.

---

## 111. Provenance in Explain Output

Can show:

```text
causation
correlation
operation lineage
authority epoch
handler/schema version
```

subject to permissions.

---

## 112. Redaction Policy

Registry Field metadata can drive redaction:

```text
Public
Sensitive
Secret
PII
Financial
```

Exact governance classes come from Part 14/29.

---

## 113. Secret Input

Never accept secrets in command arguments when avoidable because shell history/process listings can expose them.

Prefer:

```text
environment
stdin
secure prompt
secret provider
file descriptor
```

---

## 114. Authentication Token Cache

If CLI stores tokens:

```text
OS secure credential store
```

is preferred.

Plaintext config token storage should not be default.

---

## 115. Remote TLS

CLI verifies TLS normally.

No casual:

```text
--insecure
```

for production.

If development override exists, it must be visibly dev-only.

---

## 116. Profiles

Configuration profiles:

```text
dev
test
staging
production
```

can select endpoints and policies.

Profile names are not themselves security boundaries.

---

## 117. Environment Detection

Destructive dev commands should verify the target's explicit environment marker, not infer from hostname alone.

---

## 118. Auditability

Production mutating CLI actions should record:

```text
actor
AdminOperationId
command semantic action
reason
target
timestamp
outcome
```

through server audit semantics.

---

## 119. CLI Does Not Forge Audit

The server/control plane creates authoritative audit records.

The CLI merely supplies authenticated intent and reason.

---

## 120. Performance

CLI startup should remain fast.

Avoid linking every optional adapter into the default binary if it causes excessive footprint.

Use feature-selected or separate tool binaries where justified.

---

## 121. Plugin Policy

Do not introduce runtime dynamic plugins in v1.

Adapter/tool extension remains compile-time or separate executable integration.

This preserves supply-chain and ABI clarity.

---

## 122. Unsafe Code

CLI/tooling should follow workspace unsafe policy.

FFI-specific unsafe remains isolated in integration crates, not general CLI code.

---

## 123. Testing Strategy

Test layers:

```text
command parser tests
golden machine-output tests
service integration tests
real adapter tests
control-plane tests
failure/cancellation tests
security/redaction tests
cross-platform terminal tests
```

---

## 124. Parser Tests

Ensure:

```text
invalid IDs fail
conflicting flags fail
dangerous command requires required args
defaults remain stable
```

---

## 125. Golden Output

Machine schemas get golden fixtures.

Human formatting should not be treated as a stable API unless explicitly promised.

---

## 126. Secret Leak Test

Inject fake secrets into:

```text
config
error source
HTTP response
DB error
```

and verify they never appear in ordinary CLI output/logs.

---

## 127. Ctrl-C Test

Interrupt:

```text
migration planning
snapshot creation
admin apply
sync wait
```

at meaningful points and verify durable state semantics.

---

## 128. Concurrent CLI Test

Run two mutating commands concurrently.

Server/store fencing and idempotency must prevent unsafe races.

---

## 129. Store Ownership Test

With desktop agent active, direct unsafe store mutation must fail or route through agent.

---

## 130. Compatibility Test

Old CLI against newer compatible server and vice versa should produce precise negotiation results.

---

## 131. Cross-Platform Test Matrix

```text
Linux
Windows
macOS
```

The CLI should behave consistently in:

```text
TTY
non-TTY
PowerShell
POSIX shell
CI
```

---

## 132. Packaging

Part 43 will define release packaging in detail.

The CLI should ultimately be distributable as:

```text
single native binary where practical
package-manager artifacts
signed release assets
```

---

## 133. Documentation

Generate/reference:

```text
command reference
examples
exit codes
machine schemas
security model
recovery guides
```

---

## 134. Runbook Integration

Operational runbooks should invoke exact CLI commands.

Example:

```text
Authority failover runbook
1. authority status
2. promotion plan
3. review continuity
4. apply plan
5. verify epoch
```

---

## 135. Shell Script Safety

Examples should use:

```text
set -euo pipefail
```

where POSIX shell is shown and appropriate, but the architecture should not depend on shell-specific behavior.

---

## 136. Rust API Reuse

The CLI should reuse the same typed Rust clients used by:

```text
Dioxus
admin UI
automation
tests
```

rather than maintaining bespoke HTTP request code per command.

---

## 137. Local IPC Reuse

Likewise desktop CLI should reuse the same authenticated IPC protocol as other local agent clients.

---

## 138. CLI Core as Library

`aequora-cli-core` can expose:

```text
typed command execution
output model
policy metadata
```

This makes commands testable without spawning subprocesses.

---

## 139. Thin Binary

`apps/aequora/src/main.rs`:

```text
parse
initialize logging
load environment
execute typed command
render result
map exit code
```

Nothing more.

---

## 140. Example Main

```rust
#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    match aequora_cli::run(cli).await {
        Ok(output) => render(output),
        Err(error) => render_error(error),
    }
}
```

---

## 141. Structured Result

Commands return domain output:

```rust
enum CommandOutput {
    Doctor(DoctorReport),
    Sync(SyncReport),
    Registry(RegistryReport),
    Migration(MigrationReport),
}
```

Renderer decides human/JSON/RON representation.

---

## 142. Separation of Execution and Rendering

This enables:

```text
unit tests
GUI reuse
machine output
future TUI
```

without duplicating command semantics.

---

## 143. Optional TUI

A future terminal UI can sit over `aequora-cli-core`.

Do not build correctness into terminal widgets.

---

## 144. Development Server

`aequora dev server` can compose:

```text
Axum
local/test Postgres
development auth
test registry
```

but must be explicitly non-production.

---

## 145. Disposable Environments

Testkit can create:

```text
temporary local stores
temporary Postgres schemas/databases
test authority
fake transport
```

for examples and conformance.

---

## 146. Fault Injection

Development/conformance mode may expose test-only failpoints:

```text
crash after journal append attempt
network drop
serialization retry
disk-full simulation
```

These must never be enabled accidentally in production builds.

---

## 147. Feature Gates

Use Cargo features/build profiles to exclude dangerous test failpoints from production artifacts.

---

## 148. Benchmark Commands

Part 46 owns performance architecture, but tooling can expose:

```text
aequora dev bench exchange
aequora dev bench local-write
aequora dev bench reconcile
```

for controlled environments.

---

## 149. Capacity Probe

A production-safe capacity command must be read-only unless explicitly running a controlled load test environment.

Never benchmark a production authority implicitly.

---

## 150. Migration Generation

Possible:

```text
aequora migrate new <name>
```

generates:

```text
stable migration ID
RON metadata
SQL/Rust migration skeleton
test skeleton
```

---

## 151. Registry Reservation

`registry reserve` prevents developers from manually guessing durable numeric IDs.

This reduces collisions in large teams.

---

## 152. Operation Scaffolding

Possible:

```text
aequora dev operation new UpdateStudentAddress
```

generates:

```text
registry entry
typed operation skeleton
handler skeleton
tests
documentation stub
```

without filling business semantics automatically.

---

## 153. Adapter Scaffolding

Possible:

```text
aequora dev adapter new mydb
```

generates:

```text
adapter descriptor
capability manifest
trait skeletons
conformance harness
documentation template
```

---

## 154. Extension Governance

Generated scaffolding must still pass:

```text
registry review
conformance
license checks
security checks
```

Generation is convenience, not certification.

---

## 155. Supply-Chain Commands

Part 49 owns policy, but CLI/dev scripts may orchestrate:

```text
cargo audit
cargo deny
SBOM generation
license report
```

without replacing those tools.

---

## 156. Offline/Air-Gapped Tooling

Support:

```text
local registry files
local conformance
local incident inspection
local migration planning
```

without mandatory cloud services.

---

## 157. No Telemetry Requirement

CLI functionality must not depend on external telemetry.

If usage telemetry ever exists, it should be explicit and privacy-governed.

---

## 158. Developer Toolchain Invariants

### AEQ-INV-CLI001

```text
The CLI cannot bypass Aequora domain, authority, idempotency, migration, or control-plane invariants through direct internal-state mutation.
```

### AEQ-INV-CLI002

```text
Machine-readable command output is versioned and does not require scraping human terminal formatting.
```

### AEQ-INV-CLI003

```text
Secrets and registry-classified sensitive values are redacted by default from output, logs, diagnostics, and errors.
```

### AEQ-INV-CLI004

```text
Dangerous production mutations use typed authorization and explicit plan/apply semantics where required; interactive confirmation alone is never the safety boundary.
```

### AEQ-INV-CLI005

```text
A CLI timeout, cancellation, or lost response never implies that a durably submitted operation did not execute.
```

### AEQ-INV-CLI006

```text
Local store inspection respects process ownership, leases, and fencing; a CLI cannot create an uncontrolled second writer.
```

### AEQ-INV-CLI007

```text
Migration tooling verifies migration identity, checksum, source state, and required capabilities before application.
```

### AEQ-INV-CLI008

```text
Conflict, repair, authority, and consumer cursor changes occur through their registered semantic/control operations rather than direct metadata edits.
```

### AEQ-INV-CLI009

```text
Development-only destructive or fault-injection facilities cannot be accidentally enabled against a production profile/build.
```

### AEQ-INV-CLI010

```text
CLI and developer tooling reuse canonical SDK/control-plane semantics instead of independently reimplementing synchronization rules.
```

---

## 159. Recommended v1 Commands

Keep GA v1 focused:

```text
aequora init
aequora doctor
aequora config check
aequora sync status
aequora sync now
aequora operation list/show
aequora conflict list/show
aequora registry verify/diff/generate
aequora migrate status/plan/apply/verify
aequora adapter describe/verify
aequora conform
aequora diagnostics summary
aequora incident collect/inspect
aequora version
```

Advanced authority/repair/admin commands can mature behind explicit stability gates.

---

## 160. Completion Criteria

```text
[ ] command hierarchy defined
[ ] typed parser model defined
[ ] human/machine output separated
[ ] stable exit/error semantics defined
[ ] doctor architecture defined
[ ] config inspection/redaction defined
[ ] sync/operation/conflict commands defined
[ ] registry/schema tooling defined
[ ] migration plan/apply defined
[ ] adapter/conformance commands defined
[ ] snapshot/integrity/repair tooling defined
[ ] jobs/consumers/authority tooling defined
[ ] diagnostics/incident workflow defined
[ ] admin plan/apply safety defined
[ ] local store ownership rules defined
[ ] CI/noninteractive mode defined
[ ] scaffolding/devtools defined
[ ] cross-platform behavior defined
[ ] tests defined
[ ] CLI invariants defined
```

---

# 161. Final Architecture

```text
                      Developer / Operator / CI
                                |
                                v
                         `aequora` CLI
                                |
                   +------------+-------------+
                   |                          |
                   v                          v
            aequora-cli-core             Renderer
                   |                  human/json/ron
                   |
       +-----------+------------+----------------+
       |                        |                |
       v                        v                v
 Public Client SDK      Control-Plane SDK   Dev/Test APIs
       |                        |                |
       v                        v                v
 Local Client/Agent        Axum Admin       Registry /
       |                    Boundary         Conformance
       v                        |
 Durable Local State            v
                         Server Core
                                |
                                v
                       Authoritative Store
```

---

# 162. Final Recommendation

The Aequora CLI should become the safest and fastest way for developers and operators to understand the system.

It should make the correct path easy:

```text
inspect
explain
plan
verify
apply through typed APIs
re-check
```

and make dangerous shortcuts difficult or impossible.

> **A good Aequora developer tool does not give the operator permission to violate invariants; it gives the operator enough visibility and typed control to solve problems while preserving them.**

This makes the same toolchain useful from a developer laptop through CI, staging, production operations, adapter certification, incident response, and long-term ecosystem development.

---

## Next

**Part 42 — Configuration, Secrets, Environment Profiles, Runtime Policy, and Feature-Flag Architecture**
