# Public Rust SDK

The governing design is [Part 35](../sys-arch/35-public-rust-api-sdk-stability-architecture.md). This guide covers the stable application boundary; internal engine and adapter documentation remains in the owning crates.

## Client quickstart

Applications add `aequora` plus explicit storage and transport adapters. The public builder requires a cancellation-safe `ClientStore`, bounded `SyncTransport`, `CredentialProvider`, explicit tenant/device identity, and application registry `DomainId`.

```rust,ignore
use aequora::prelude::*;

let client = AequoraClient::builder()
    .store(store)
    .transport(transport)
    .credentials(credentials)
    .identity(identity)
    .domain(DomainId::new("school")?)
    .build()
    .await?;

let receipt = client.mutate(CreateStudent { student_id, name }).await?;
println!("saved locally as {}", receipt.operation_id());

let result = client.sync_now().await?;
```

`MutationReceipt` proves only local durable commit. Authoritative acceptance is queried through `client.operations()` or observed as an advisory `SyncEvent::OperationUpdated`. Dropping an SDK future cannot roll back intent that an adapter already made durable.

## Events and current state

`client.events()` is a bounded best-effort UI notification stream. A slow subscriber may miss advisory events. Durable truth remains available through `client.status()`, `client.operations()`, and the conflict/scope handles. UI correctness must query current state after startup, resume, or suspected lag.

## Grouped capabilities

- `client.operations()` inspects durable operation state and offers bounded authoritative waiting.
- `client.conflicts()` lists and resolves semantic conflicts without exposing ledger rows.
- `client.scopes()` persists subscription intent without exposing cursor mutation.
- `client.blobs()` is capability-gated and returns `UnsupportedCapability` when no provider exists.
- `client.diagnostics()` returns bounded, redacted summaries.

## Server quickstart

```rust,ignore
use aequora::prelude::*;

let domain = DomainRegistry::builder()
    .register(create_student_handler)
    .build()?;

let server = AequoraServer::builder()
    .authority_store(authority)
    .registry(domain)
    .authenticator(authenticator)
    .build()?;
```

Duplicate operation kinds, missing schemas, and missing consistency profiles fail before traffic. Domain handlers receive validated semantic context, not HTTP headers, wire framing, or database transactions. Axum and other transports adapt to this server core from their own crates.

## Errors

`AequoraError` categories are non-exhaustive. Callers should branch on `error.code()` and `error.retry_class()` with a catch-all. Error strings are developer diagnostics, not localized UI copy. Credential `Debug` output is always redacted.

## Extension contract

Third-party integrations implement traits from `aequora-adapter-sdk`. These traits are intentionally open only for storage, transport, credentials, domain handling, authority commit, authentication, and observability. Cursor ordering, authority epochs, identity, and idempotency are closed core semantics.

## Threading and lifecycle

`AequoraClient` is cheaply cloneable, `Send + Sync`, and every clone refers to one runtime. Dropping one clone does not shut it down. `shutdown().await` is explicit, but crash correctness never depends on graceful shutdown. Observability callbacks must be non-blocking and are never invoked while holding a public application lock.
