# Restore and authority promotion

1. Stop routing writes and fence the old database writer at the infrastructure and Aequora layers.
2. Restore or promote the candidate in isolation. Verify ledger, journal, timeline head, snapshots,
   migrations, registry, and business/audit state.
3. Prove whether every acknowledged commit exists on the candidate. If this cannot be proved,
   establish a new `AuthorityEpoch` before any synchronization resumes.
4. Publish the authority endpoint only after readiness passes. DNS or traffic-manager changes alone
   never grant authority.
5. Reconnect clients using their original `OperationId`s, monitor epoch mismatches and duplicate
   outcomes, and retain all promotion evidence in durable audit.

