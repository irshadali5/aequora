# Part 47 benchmarking and capacity-planning completion

The Part 47 architecture is implemented as three neutral layers. `aequora-workload` owns seeded,
versioned, bounded workload descriptions and the eight recommended v1 scenario artifacts.
`aequora-benchkit` owns immutable run manifests, correctness profiles, tail-latency and resource
results, compatibility-aware regression decisions, production-target guards, and capacity
estimates whose evidence is explicitly measured, interpolated, extrapolated, or unknown.
`aequora-loadgen` owns bounded stateful virtual clients, visible offered/admitted/accepted/rejected
accounting, and an incremental correctness oracle. Runtime adapters can drive actual HTTP,
PostgreSQL, SQLite, Stoolap, device, multi-node, or network-emulation runs without coupling those
products to the reusable contracts.

The implementation deliberately publishes no capacity number. The CLI reports `Unknown` and
`Not Yet Certified` until an environment-bound result bundle exists; even a measured estimate is
not marked certified by the reusable estimator. Production execution is denied unless a host
supplies an allowlist, rate cap, isolated tenant, and operator confirmation.

Executable evidence includes `AEQ-INV-BENCH001` through `AEQ-INV-BENCH010`, the
`BenchmarkingFull` conformance profile and tests 154–163, the Part 47 testkit contract, and
`scripts/check-benchmarking-architecture.sh`. The architecture gate verifies the eight workload
files, neutral dependency boundary, CLI safety behavior, registry parity, focused unit tests, and
conformance/invariant coverage.

Dedicated PostgreSQL/Neon scalability, soak, failure-degraded, real-device mobile, multi-region,
air-gapped hardware, flamegraph, heap, and release-capacity runs remain environment-bound evidence.
They cannot truthfully be manufactured by a repository-only test. Their results fit the implemented
manifest and result schemas and may be attached to Part 30 certification only after execution in a
controlled environment.
