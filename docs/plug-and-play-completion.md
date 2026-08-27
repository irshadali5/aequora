# `plug-and-play.md` implementation completion

This record maps the reusable integration architecture to the public workspace. It does not turn
optional tooling examples into claims about commands or templates that do not yet exist.

## Section map

| Sections | Area | Current evidence | Status |
|---|---|---|---|
| 1–17 | SDK layers, facade, preludes, modes, migrations, domain operations | `aequora` facade, focused client/server preludes, stable builder entry points, built-in adapter migrations, typed operation model | Implemented |
| 18–24 | derive and registration ergonomics | `AequoraOperation`, `OperationHandler`, `OperationRegistry`, duplicate-kind rejection | Implemented; registration macro intentionally unnecessary |
| 25–39 | entity/repository/transaction/auth/conflict/upcasting integration | opaque repositories, application transaction hooks, scope authorization, conflict registry, `PayloadMigrator` | Implemented at framework boundary |
| 40–53 | codecs, routes, middleware/auth, identity, lifecycle, status, extensions | Postcard default, optional RON/JSON, Axum installers, injected auth, coordinator status/watch APIs, observers; no dynamic plugins | Implemented |
| 54–61 | features, templates, init/doctor/migrate/inspect | orthogonal facade features, non-destructive compile-checked client/server init, static payload-free doctor/inspect/pair CLI, and verified canonical artifact inspection | Implemented at the safe generic boundary |
| 62–74 | TestKit, faults, adapter and transport SDK | deterministic stores/transports, failpoints, simulation, shared conformance functions and factory macros, manifests, normalized errors | Implemented |
| 75–89 | framework independence, module boundaries, protocol inventory, config/profiles | generic exchange service, no Dioxus dependency, typed registry, protocol compatibility, strict RON and deployment profiles | Implemented |
| 90–103 | bootstrap/startup, scopes, builders, validation, errors | automatic migrations, snapshot bootstrap, type-state builders, typed errors, explicit `build_production` adapter verification | Implemented |
| 104–119 | docs/examples/tests/ecosystem/deprecation/code generation | runnable examples, integration/property/model tests, rustdoc and semver metadata; generated starter sources are workspace example targets | Implemented at the repository boundary |
| 120–145 | security defaults, ownership, public surface, examples, final contract | bounded protocol/config, explicit auth, small facade, `AequoraClient`/`AequoraServer`, Stoolap-to-PostgreSQL examples | Implemented at core SDK boundary |

## Initial deliverable status

| Deliverable from section 142 | Status |
|---|---|
| facade, builders, handlers, registry | Implemented |
| operation derive | Implemented as `AequoraOperation` |
| automatic Stoolap/PostgreSQL metadata migrations | Implemented |
| Axum route installer and auth bridge | Implemented at framework boundary |
| status subscription and TestKit | Implemented |
| adapter compliance | Implemented as public functions and factory-driven test macros |
| protocol compatibility checker | Implemented in validator/server capability negotiation |
| doctor and inspect | Implemented for static payload-free adapter facts |
| canonical migration artifact | Implemented with bounded source paging, verified chunks, and resumable atomic sink contracts |
| non-destructive project templates | Implemented; exact generated source compiles as workspace examples |
| migration CLI | Implemented for safe offline artifact verification; live execution is host/adapter-owned |

## Prerequisite closure and ownership

The repository-owned plug-and-play prerequisite is complete. `inspect adapters` generates the
static support data from manifests, `verify export` validates schema identity and every artifact
digest, and exact client/server starter sources are compiled as workspace example targets. Package
acceptance must additionally compile them against the published facade rather than workspace paths.

Live schema health and migration execution remain host integrations: the host supplies concrete
connections, authentication, canonical mappings, source snapshot policy, and destructive-operation
authorization. Keeping those inputs out of the generic CLI prevents credential leakage and avoids
pretending that business-table mappings can be inferred safely.

Dynamic plugins, ORM generation, conflict inference, and automatic business authorization remain
deliberately delayed as specified by the architecture.

## Verification

```bash
cargo test -p aequora --test derive_operation
cargo test -p aequora-store -p aequora-client -p aequora-server -p aequora-cli --all-targets
cargo check -p aequora --all-features --all-targets
```
