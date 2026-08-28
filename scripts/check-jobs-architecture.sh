#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-JOB${suffix}" crates/aequora-invariants/src/lib.rs
done

for crate in aequora-jobs aequora-workflow aequora-side-effects; do
    test -f "crates/${crate}/Cargo.toml"
    test -f "crates/${crate}/src/lib.rs"
    if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|reqwest|rayon)([^[:alnum:]_-]|$)' \
        "crates/${crate}/Cargo.toml"; then
        echo "jobs architecture: runtime, database, transport, or provider dependency in ${crate}" >&2
        exit 1
    fi
done

for symbol in JobStore JobLease FencingToken RetryPolicy RecurringSchedule \
    JobEpochPolicy JobGovernanceMetadata JobAccessPolicy JobAdminService; do
    rg -q "${symbol}" crates/aequora-jobs/src crates/aequora-server/src/jobs.rs
done

for symbol in WorkflowDefinition WorkflowTransition ScheduleCompensation WorkflowStore; do
    rg -q "${symbol}" crates/aequora-workflow/src
done

for symbol in SideEffectIntent SideEffectIntentWrite ExternalIdempotencyKey \
    AmbiguousRecoveryPolicy AuthoritativeOperationSink SideEffectProvider; do
    rg -q "${symbol}" crates/aequora-side-effects/src
done

test -f crates/aequora-testkit/tests/job_workflow_side_effect_contracts.rs

echo "jobs architecture: durable jobs, fenced workflows, typed side effects, nine invariants, and neutral dependencies verified"
