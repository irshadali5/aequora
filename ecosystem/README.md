# Aequora certification ecosystem

The catalog begins empty intentionally: inclusion requires a content-addressed artifact produced
for the exact subject binary, feature set, suite version, and environment. Passing ordinary unit
tests or being maintained in this repository is not itself a certification.

Trust labels mean:

- `Experimental`: evidence is incomplete or intended only for development.
- `CommunityVerified`: a community verifier reproduced the published artifact.
- `MaintainerVerified`: Aequora maintainers reproduced it against the governed suite.
- `Official`: maintainers publish and support the implementation and its signed evidence.

Lifecycle values are `Active`, `Superseded`, `Suspended`, and `Revoked`. Status changes retain the
original certification identity and reason. Historical artifacts remain immutable and
verifiable. Production policy rejects inactive, mismatched, below-tier, below-trust, or obsolete
suite evidence; development policy may emit a visible warning instead.

Catalog submissions must include license, security contact, SBOM digest, provenance digest,
supported versions/environments, limitations, and reproducible commands. Vulnerability reports
can suspend or revoke affected records without erasing history. See
`docs/certification-security-advisories.md`.
