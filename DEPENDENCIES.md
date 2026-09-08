# Dependency policy

Every dependency expands Aequora's legal, security, maintenance, portability, and reproducibility
boundary. New dependencies need a capability justification, verified package identity and license,
architecture layer, feature/native/build-script/proc-macro impact, owner, replacement plan, and
verification appropriate to the risk tier.

Tier 2 and Tier 3 changes must not merge or ship without explaining their lockfile and transitive
diff and running the relevant compatibility, conformance, migration, security, and performance
checks. Unknown licenses and unapproved sources fail closed. Exceptions require an owner, reason,
evidence, and expiry.

Machine-readable policy lives in supply-chain/. The resolved graph is governed by deny.toml,
Cargo.lock, the Part 49 architecture gate, RustSec audit CI, and per-artifact SBOM generation.
