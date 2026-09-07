# Rolling upgrade

1. Verify backup/PITR and a recent isolated restore drill.
2. Verify release signatures, hashes, registry/config digests, and compatibility dimensions.
3. Acquire the single migration-owner lock and apply expand-only schema work.
4. Deploy a bounded canary, verify readiness and protocol overlap, then roll remaining API nodes.
5. Drain workers using durable lease fencing; never rely on process-local ownership.
6. Run topology doctor and bootstrap/sync smoke tests before completing deferred contract steps.
7. Roll back only when the release manifest and migration evidence say current state is compatible.

