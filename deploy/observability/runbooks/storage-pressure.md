# AEQ-ALERT-STORAGE-001 — storage pressure

Measure journal, ledger, snapshot, local outbox, staging, and optional log/cache growth separately.
Rotate optional logs and purge rebuildable cache first. Never evict pending intent, cursors, conflicts,
authority metadata, or required audit. Apply retention only below proven safety floors.

