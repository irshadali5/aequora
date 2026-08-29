# Security Ownership and Release Gate

Security-critical APIs require review from the subsystem owner and a reviewer independent of the
author for high-risk changes. Repository maintainers assign people/teams in deployment governance;
this file records code ownership boundaries without inventing personal names.

| Boundary | Security-sensitive APIs | Owning subsystem | Required reviewer |
|---|---|---|---|
| Authentication and tenant binding | `AuthenticationPolicy`, `ValidatedAuthContext`, `AuthContext::from_validated_security` | security/executor | identity + domain authorization |
| Protocol/parser | envelopes, codecs, compatibility capability classes, limits | protocol/codec/compat | security + protocol |
| Cryptography/secrets | providers, key registry, policy, artifact verification | crypto | cryptography-qualified reviewer |
| Authority/replica | promotion, fencing, epoch/checkpoint verification, regional routing | authority/region | distributed-systems + security |
| Admin/break-glass | permissions, plans, approvals, destructive execution, audit | admin/server | operations + security; separation of duties |
| Jobs/providers/payments | intents, fencing, idempotency, reconciliation, egress targets | jobs/side-effects/security | domain owner + security |
| Governance/audit | hold, erasure/purge, restore, append-only evidence | governance/audit | governance + security |
| Import/legacy/diagnostics | parsers, quarantine, cutover, bundle publication/replay | migration/legacy/diagnostics | subsystem + security |
| Storage adapters | tenant-scoped queries, atomic ledger/journal/audit commit, DB roles | store adapters | adapter + security |
| Public/admin HTTP | authentication middleware, limits, cookies, CSRF/CORS/headers | transport/application host | security + deployment owner |

Before production release, record evidence that dependency advisories were reviewed, the threat
matrix covers changed entry points, auth/authz and abuse tests pass, fuzz targets are healthy, secret
scan is clean, admin permissions were reviewed, protocol limits match deployment, TLS/private object
storage/KMS/DB-role controls are active, and high-risk changes received required review. Manual
penetration testing covers API, admin, authentication, webhooks, and uploads before a major release.

If unsafe code enters any security-critical crate, isolate the boundary and require separate review,
fuzzing, and Miri where applicable. The workspace currently forbids unsafe code. Real bug-derived,
sanitized fuzz inputs and vulnerability regressions remain in their respective corpus/test suites.
