# Air-gapped release import

1. Build once in the protected release workflow and export the signed release manifest, hashes,
   registry snapshot, migration bundle, SBOM, and platform artifacts.
2. Transfer the immutable bundle under the organization's media-control procedure.
3. In a quarantined verification host, validate release signatures, purpose-specific trust roots,
   revocation metadata, and every artifact hash with `aequora release verify`.
4. Import verified bytes into the internal repository; never rebuild them inside the gap.
5. Run `aequora doctor deployment` against an `AirGapped` descriptor. It must declare no public
   dependency and must prove internal identity, DNS, trusted time, object storage, backup,
   observability, trust roots, and offline release verification.
6. Deploy by digest, run migration readiness, health, journal, and bootstrap checks, and archive the
   verification evidence in durable audit storage.

Offline revocation metadata follows the same signed-bundle process. Journal order never uses wall
clock order, but trusted internal time remains necessary for certificates and release validity.

