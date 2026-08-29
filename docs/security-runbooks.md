# Security Incident Runbooks

All runbooks start by opening an incident identity, preserving audit evidence, recording UTC
timestamps and decision owners, and avoiding destructive cleanup until evidence is protected. Use
Part 25 bundles only through authorized, sanitized, encrypted production policy. Never copy tokens,
private keys, raw credentials, or unrestricted tenant payloads into tickets or chat.

## Stolen or compromised device

1. Revoke the device and active sessions; reject new sync and operations.
2. Identify affected tenants, scopes, token lifetime, last trusted sync, and suspicious operations.
3. Rotate refresh/device credentials and require step-up authentication for account recovery.
4. Schedule best-effort remote purge on reconnect; state explicitly that offline plaintext may remain.
5. Reconcile operation IDs and close only after revocation tests and audit evidence verify.

## Compromised key or credential

1. Stop new use, mark the exact key/secret version revoked, and activate a purpose-correct replacement.
2. Rotate dependent service credentials without deleting historical public verification metadata.
3. Determine tenants, artifacts, time window, and signing/encryption purpose exposed.
4. Re-sign/re-encrypt only through governed migration; assess all recoverable key copies before any
   cryptographic-erasure claim.
5. Verify registry state, audit checkpoints, provider credentials, and alert thresholds.

## Suspected database tampering or authority fork

1. Stop/fence writes when continuing could extend a fork; do not auto-merge histories.
2. Compare authority identity, highest epoch, journal/audit checkpoints, and external anchors.
3. Preserve database, WAL/backup, node, and bundle evidence under legal/governance policy.
4. Promote or restore only through Part 16 reviewed evidence and a new epoch unless continuity is
   positively proven.
5. Force affected clients to rebootstrap and verify audit/journal roots before resuming traffic.

## Provider, payment, or webhook incident

1. Open the circuit breaker and suspend new irreversible effects for the affected integration.
2. Rotate webhook/provider credentials; keep inbound events quarantined and bounded.
3. Reconcile every ambiguous request by stable idempotency key and provider reference before retry.
4. Reject client amounts, source-IP-only trust, stale signatures, duplicates, and private egress targets.
5. Resume gradually after signature, SSRF, duplicate, and reconciliation acceptance tests pass.

## Admin credential compromise

1. Revoke sessions, break-glass grants, workload credentials, and approval authority.
2. Isolate the private control-plane listener and stop destructive admin execution if attribution is
   uncertain.
3. Review immutable admin operations, payload digests, plans, approvals, postconditions, and security
   events for the exposure window.
4. Rotate credentials, require fresh MFA, restore separation of duties, and independently verify state.

## Legacy compromise or post-cutover write

1. Pause the bridge and fence the legacy writer; treat any post-cutover write as critical.
2. Preserve CDC cursor/source evidence and quarantine untrusted records.
3. Re-run typed mapping, final-boundary, shadow, and canonical verification before resume.
4. Reconcile governance coverage for all legacy copies and keep the identity map required for audit.

## Split brain or resource-exhaustion attack

For split brain, fence all but the verified authority, preserve checkpoints, and follow the database
tampering runbook. For overload, reject before mutation, preserve per-tenant fairness, shed only
non-durable work, retain jitter, and never weaken authorization, consistency, governance, or required
security capabilities. Escalate edge/network DDoS controls without turning an incident bypass into a
permanent configuration.
