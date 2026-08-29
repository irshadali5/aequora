# Conformance invariant coverage matrix

The stable invariant registry in `aequora-invariants` is normative. This matrix identifies the
primary executable evidence family for every architecture part; certification artifacts retain
the exact invariant IDs and test IDs they exercised. Manual and deployment evidence cannot replace
required automated correctness tests.

| Part | Primary verification | Executable evidence |
|---:|---|---|
| 01 | model + automated | `aequora-model`, `verify model` |
| 02 | automated | lineage and audit contract tests |
| 03 | property + automated | integrity and repair contracts |
| 04 | property + automated | queue compaction/rebase contracts |
| 05 | concurrency + automated | coordination and fencing contracts |
| 06 | property + automated | scheduler QoS contracts |
| 07 | automated | scope transition contracts |
| 08 | automated | live hint loss/backpressure contracts |
| 09 | differential + automated | migration/export contracts |
| 10 | failure injection + automated | bootstrap streaming contracts |
| 11 | property + automated | consistency profile contracts |
| 12 | replay + model | deterministic replay contracts |
| 13 | tamper + automated | audit provenance contracts |
| 14 | failure injection + automated | governance/erasure contracts |
| 15 | known-answer + automated | crypto contracts |
| 16 | model + failure injection | authority failover contracts |
| 17 | model + automated | regional routing contracts |
| 18 | property + load | admission/fairness contracts |
| 19 | benchmark + correctness | bounded performance architecture gate |
| 20 | property + resource | constrained-client contracts |
| 21 | differential + automated | compatibility negotiation contracts |
| 22 | adapter differential | metadata architecture and adapter contracts |
| 23 | failure injection + automated | jobs/workflow/side-effect contracts |
| 24 | authorization + automated | admin control-plane contracts |
| 25 | tamper + replay | diagnostic incident contracts |
| 26 | shadow + differential | legacy adoption contracts |
| 27 | abuse matrix + automated | security abuse contracts |
| 28 | failure injection + automated | change-feed contracts |
| 29 | generation + compatibility | registry lock/codegen contracts |
| 30 | harness + artifact verification | `aequora-conformance` and certification contracts |

Deployment-specific rows such as production restore, failover, external provider credentials,
database version ranges, and signed release provenance are recorded in the certification artifact
as `DeploymentValidation` or `ManualReview` evidence with limitations. They are never silently
marked automated.
