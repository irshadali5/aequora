# Aequora conformance fixture package

These versioned, payload-free fixtures exercise the public certification artifact format. Test
IDs, profile IDs, and tier IDs are governed by `registry/core/certification.ron`; existing IDs are
never reassigned. A harness must record its seed, exact binary digest, feature set, build
configuration, provider/database versions, and evidence digests.

`storage-core-passing-request.ron` is a format and CLI fixture, not a certification of a shipping
adapter. Built-in and third-party adapters must run `aequora-testkit` contracts against isolated
real instances and publish the resulting content-addressed artifact.

To evaluate the fixture:

```text
aequora conform run conformance/fixtures/storage-core-passing-request.ron
```

Correctness tests are release gates. Benchmarks and performance characterization are separate and
cannot change a failed correctness result.
