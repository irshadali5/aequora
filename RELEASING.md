# Release supply-chain requirements

Production releases use the reviewed Cargo.lock with locked resolution on a controlled runner.
After dependencies and toolchains are staged, high-assurance builds run without uncontrolled
network access. Ordinary and pull-request jobs never receive release signing credentials.

Each shipped profile produces a reviewed SPDX or CycloneDX SBOM, third-party notices, build
provenance, artifact and metadata digests, full quality evidence, and signatures. Stable promotion
reuses the already verified bytes. Reproducibility is reported at the demonstrated level only;
byte-identical status requires independent matching artifact digests.

Air-gapped bundles include dependency sources, toolchain, licenses, SBOM, provenance, signatures,
checksums, and offline verification instructions.
