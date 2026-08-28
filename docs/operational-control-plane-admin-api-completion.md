# Part 24 operational control plane and admin API completion

## Implemented scope

The reusable implementation of
[`sys-arch/24-operational-control-plane-admin-api.md`](../sys-arch/24-operational-control-plane-admin-api.md)
is centered on the runtime-, transport-, authentication-vendor-, and database-neutral
`aequora-admin` crate.

Delivered contracts include:

- stable `AdminOperationId`, `PlanId`, `ApprovalId`, `MaintenanceId`, and
  `BreakGlassSessionId` identities;
- numeric permission and action registries, generic assurance levels, tenant-scoped principals,
  bounded reasons, step-up freshness, and distinct force actions;
- typed authority, job, snapshot, integrity, governance, compatibility, crypto, regional,
  maintenance, tenant, device, scope, export, diagnostics, and configuration commands;
- server-side authorization, actor-bound idempotency digests, exact plan digests, safety snapshots,
  expiry/staleness checks, second-person approvals, and verified-completion enforcement;
- durable-store, subsystem-executor, and Part 13 audit boundaries, plus a concurrency-safe
  reference store for conformance and embedding;
- maintenance-state and dynamic-policy generation compare-and-swap with bounded rollback;
- typed admin queries/read models for authority, jobs, snapshots, integrity, governance,
  compatibility, public crypto metadata, regions, health, runtime diagnostics, and operations;
- stable machine error codes and HTTP-status guidance without an HTTP dependency;
- fail-closed private-listener policy, strong-auth/permission-registry validation, restrictive
  browser-session policy, API version, capability, and wire-format contracts;
- a thin `aequora-server::admin` facade that does not couple sync requests to control-plane
  availability, and read-only `aequora admin` capability, policy, and action inspection commands;
- `AEQ-INV-ADMIN001` through `AEQ-INV-ADMIN009` in the normative invariant registry.

Long-running operations return or retain a `JobId`; application integrations implement
`AdminCommandExecutor` by delegating to the existing authority, Part 23 job, snapshot, integrity,
governance, compatibility, crypto, region, and domain-operation services. There is intentionally
no direct SQL, metadata-row mutation, private-key response type, or public admin listener in the
reusable core.

## Executable evidence

The repository contains:

- unit scenarios for no-auth/wrong-permission and tenant scope, actor-bound idempotency, changed
  payload rejection, plan expiry/staleness, staleness after an approval delay, separation of
  duties, postcondition verification, dynamic-config CAS/rollback, maintenance CAS, listener
  fail-closed behavior, and stable error mappings;
- `crates/aequora-testkit/tests/admin_control_plane_contracts.rs`, proving the server-facing plan
  and guarded execution path;
- `scripts/check-admin-architecture.sh`, checking neutral dependencies, required integration
  surfaces, private-key type exclusion, and all nine Part 24 invariant identifiers;
- CI execution of the architecture script, focused crate/contracts, and CLI capability discovery.

The final implementation run passed formatting, the focused Part 24 script and tests, all other
listed architecture scripts, all four database-neutral profiles, workspace all-target/all-feature
check, strict workspace Clippy, the full all-feature workspace test suite, and rustdoc with warnings
denied. Guppy reported 19 boundary rules across 56 workspace crates. The full test run was repeated
outside the filesystem/network sandbox because the sandbox forbids the `aequora-http` loopback
socket; the exact loopback test and then the complete workspace suite passed there. The refreshed
RAG/GraphRAG index processed all 334 repository files.

## Deployment-owned boundaries

An application or deployment still supplies the identity provider or mTLS verifier, concrete RBAC
or ABAC mapping, durable admin/audit adapter, subsystem executors, private listener and network
policy, optional Axum JSON routes/OpenAPI, CSRF/CORS middleware, telemetry/alerts, and CLI transport
credentials. Those choices are intentionally outside a database-neutral library and must satisfy
the fail-closed contracts before mutating endpoints are enabled.
