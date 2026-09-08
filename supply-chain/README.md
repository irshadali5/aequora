# Supply-chain governance

The policy file is the reviewed Tier 2 and Tier 3 dependency inventory and fail-closed
license/source policy. The repository deny configuration evaluates the complete resolved Cargo
graph. The remaining RON files retain explicit exceptions, approved sources, critical risk
decisions, and operational provider boundaries.

Release automation must generate an SBOM and dependency report for each shipped feature profile
from the same locked graph used for the artifact. The report inventories duplicate versions,
native boundaries, build scripts, proc macros, licenses, and sources. Automation binds the SBOM to
source/build/provenance digests, generates third-party notices, and validates it with the neutral
supply-chain contracts before signing.

The checked-in risk classifications are engineering reviews, not legal advice. Material license
or distribution decisions require the distributor's legal review.
