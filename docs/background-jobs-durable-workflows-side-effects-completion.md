# Part 23 completion: background jobs, durable workflows, and side effects

The governing specification is
[`sys-arch/23-background-jobs-durable-workflows-side-effects.md`](../sys-arch/23-background-jobs-durable-workflows-side-effects.md).
The implementation is split into database-neutral `aequora-jobs`, `aequora-workflow`, and
`aequora-side-effects` crates, with bounded server orchestration in `aequora-server::jobs`.

## Executable scope

| Requirement | Evidence |
|---|---|
| Durable jobs | Typed job identity/kind/state/payload records, bounded inline payloads and checkpoints, `JobStore`, status/admin contracts, and notification-independent claim requests |
| Claims and fencing | Monotonic `FencingToken`, worker-owned expiring `JobLease`, compare-and-swap `FencedUpdate`, and executable stale-worker validation |
| Retry and bulkheads | Typed error taxonomy, bounded exponential backoff with mandatory jitter, provider retry-after, concurrency classes, worker capabilities, bounded claim batches, and server-side permit-aware planning |
| Dependencies and timers | Bounded acyclic dependency validation, durable `next_run_at`, recurring schedules, missed-occurrence policy, and deterministic unique occurrence identity |
| Durable workflows | Versioned workflow records, optimistic row versions, deterministic event transitions, bounded checkpoints/fan-out, explicit compensation actions, and atomic workflow-plus-child-job transaction contracts |
| Side-effect outbox | Immutable typed intent with operation/tenant/correlation lineage, payload digest, stable external idempotency key, and an authoritative-transaction write trait |
| Provider ambiguity | Explicit external outcomes, provider capability certification, reconcile/same-key/manual/suppress policies, and provider reconciliation contracts |
| Authoritative result path | `AuthoritativeResultOperation` and `AuthoritativeOperationSink` require provider results that affect business state to re-enter the authoritative handler pipeline |
| Epoch and region recovery | Authority-epoch recovery actions and authority-only/allowed/specific-region placement policies |
| Governance | Retention class, PII/legal-hold metadata, subject/artifact references, and side-effect retention metadata cover pending jobs and external intents |
| Operations | Tenant-authorized status/admin service, supported-kind/capability-aware claiming, bounded polling inputs, graceful stop-claiming state, and stable diagnostic invariant entries |
| Correctness registry | `AEQ-INV-JOB001` through `AEQ-INV-JOB009` in the workspace invariant registry |

## Responsibility boundary

The core crates deliberately do not open a database, run Tokio workers, call a provider SDK, or
claim exactly-once delivery. Physical adapters must implement the store traits with atomic insert,
claim, lease, compare-and-swap, uniqueness, indexes, and transaction participation appropriate to
their database. Provider integrations live outside these crates and must honor declared timeout,
idempotency, lookup, egress, secret-reference, and reconciliation policy.

Application handlers own job kinds, payload schemas, business cancellation semantics, workflow
versions, authoritative result operations, retention policy values, provider allowlists, and admin
authorization. Deployment-specific load tests and provider certification remain deployment
evidence, while the reusable safety contracts and failure models are executable here.

## Verification

```text
CARGO_BUILD_JOBS=1 cargo test -p aequora-jobs -p aequora-workflow -p aequora-side-effects
CARGO_BUILD_JOBS=1 cargo test -p aequora-testkit --test job_workflow_side_effect_contracts
CARGO_BUILD_JOBS=1 cargo clippy -p aequora-jobs -p aequora-workflow -p aequora-side-effects -p aequora-server --all-targets -- -D warnings
bash scripts/check-jobs-architecture.sh
cargo run -q -p aequora-dev -- check
bash scripts/check-database-neutrality.sh
```

The focused suite covers lease expiry/reclaim fencing (including generated stale tokens), cyclic
dependencies, provider timeout ambiguity, stable-key idempotent retry, retry backoff, workflow
compensation, duplicate recurring schedulers, authority-epoch recovery, and governance coverage.
