# AEQ-ALERT-AUTHORITY-001 — authority commit failure

Verify authority role/epoch, PostgreSQL reachability, pool wait, transaction failures, journal append,
and recent migrations. Stop unsafe writes if continuity is uncertain. Do not retry with a new
`OperationId`. Shed optional work, restore database capacity, then verify ledger/journal consistency
and a known-operation replay before closing the incident.

