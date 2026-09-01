# `database-interoperability.md` implementation completion

This is the implementation record for the 200-section universal database architecture. The
architecture document remains normative. This record distinguishes the preferred domain-operation
mode from optional generic-record, legacy CDC, and database-migration tooling.

## Section map

| Sections | Area | Current evidence | Status |
|---|---|---|---|
| 1–8 | universal architecture and canonical data plane | `OperationEnvelope`, `SyncRequest`, `SyncResponse`, and store/transport traits contain no database schema or SQL | Implemented |
| 9–16 | canonical values and database capabilities | opaque domain payloads plus feature-gated exact canonical values, typed IDs, transaction declarations, snapshots, and capability flags | Implemented |
| 17–22 | adapter roles and local/authority contracts | `AdapterRole`, `LocalStore`, `AuthoritativeStore`, `TransactionCapabilityProvider`, and independent wrapper adapters | Implemented |
| 23–43 | schema/type mapping and database-pair shapes | application handlers map operations; `aequora-schema` and `aequora-mapping` validate explicit record-mode maps; `CanonicalValueConverter` and golden fixtures certify adapter conversions without pair-specific protocol logic | Implemented at the reusable boundary |
| 44–49 | certification, manifest, startup negotiation | `AdapterTier`, `AdapterManifest`, `AdapterRequirements`, and `ProductionAdapterPair`; built-in manifests fail closed | Implemented |
| 50–67 | native optimization, CDC, snapshots, projections, store migration | native Stoolap/PostgreSQL implementations, staged snapshots, and projection hooks exist | Core snapshot/projection path implemented; CDC and store-migration workflow pending |
| 68–79 | adapter SDK, transactions, types, query/repository independence | narrow async traits, application transaction hooks, normalized errors, opaque payloads | Implemented for domain mode |
| 80–91 | generic CRUD, schema registry, field IDs, mapping validation | stable entity/field IDs, validated schema registry, physical schema revision, explicit field/type mapping and loss policy | Schema/mapping implemented; generic CRUD intentionally remains adapter/application-owned |
| 92–123 | source/sink roles, feeds, conformance, roundtrips, export/import | `CanonicalRecordSource`/`CanonicalRecordSink`, bounded paging, resumable idempotent staging, atomic publication contract, schema-identified chunks, tamper/root verification, and executable source-to-sink roundtrip | Implemented at the reusable boundary |
| 124–150 | discovery, support priority, health, errors, runtime registries | statically linked manifests, payload-free CLI doctor/inspect/pair verification, database health checks, retry classification, Stoolap local and PostgreSQL authority | Static v1 implemented; runtime multi-store registry pending |
| 151–176 | modular stores, performance, serialization, support matrices | independent crates, bounded batches, prepared PostgreSQL queries, Postcard/RON/JSON boundaries, and machine-readable `inspect adapters` output generated from manifests | Implemented where current adapters apply |
| 177–200 | ecosystem layout, selection, migration/repair tooling, support strategy | optional schema/mapping/migration crates, facade `record-sync` feature, verified export artifacts, source/sink orchestration, static dispatch, custom adapter contracts, Stoolap/PostgreSQL path, and CLI artifact verification | Implemented at the reusable boundary |

## Built-in support matrix

| Adapter | Local | Authority | Tier | Tested database | Snapshot role | Manifest |
|---|---:|---:|---|---|---|---|
| Stoolap | Yes | No | Full production | 0.4.0 | Sink | `STOOLAP_ADAPTER_MANIFEST` |
| PostgreSQL | No | Yes | Full production | 18 | Source | `POSTGRES_ADAPTER_MANIFEST` |
| TestKit in-memory | Reference only | Reference only | Not production | process memory | Both | Test-only semantics |

Part 42 now supplies the official SQLite local-replica adapter. This table does not claim Redb,
MySQL, document, graph, or key-value adapters. Those engines
become supported only after a concrete adapter publishes a truthful manifest and passes the
relevant real-engine contracts.

The same support data is generated directly from the built-in manifests with:

```bash
cargo run -q -p aequora-cli -- inspect adapters
```

## Prerequisite closure and ownership

The repository-owned prerequisite is complete. Concrete record conversion, consistent snapshot
selection, and atomic target publication are necessarily implemented by each application adapter
because Aequora does not know its business tables. Adapter authors use `CanonicalValueConverter`,
golden `ConversionFixture` values, `CanonicalRecordSource`, and `CanonicalRecordSink`; the shared
orchestration rejects invalid paging, schema drift, tampering, incomplete staging, and undeclared
conversion loss.

`aequora verify export` is deliberately offline and credential-free. Opening a live database,
choosing application records, resetting an authority timeline, or overwriting a target requires an
explicit host command around a concrete source/sink. Built-in health APIs can be exposed by that
host after it supplies connections. No generic CLI command guesses mappings or accepts database
credentials on Aequora's behalf.

CDC bridges, graph mapping, generic CRUD, runtime-loaded adapters, and automatic semantic mapping
remain optional and must not enter the core synchronization hot path.

## Verification

```bash
cargo test -p aequora-store -p aequora-store-stoolap -p aequora-store-postgres --lib
cargo test -p aequora-schema -p aequora-mapping -p aequora-migration -p aequora-cli
cargo check -p aequora --all-features --all-targets
cargo run -q -p aequora-dev -- check
bash scripts/check-database-neutrality.sh
```

Live PostgreSQL/Neon conformance still requires the configured environment URLs and is not claimed
by an offline run.
