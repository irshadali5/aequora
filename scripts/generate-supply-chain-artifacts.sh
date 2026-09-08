#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output="${1:-$root/target/supply-chain}"
artifact="${2:-$root/Cargo.lock}"
mkdir -p "$output"

metadata="$(mktemp)"
trap 'rm -f "$metadata"' EXIT

cd "$root"
CARGO_BUILD_JOBS=1 cargo metadata --format-version 1 --locked --offline >"$metadata"

artifact_sha256="$(sha256sum "$artifact" | awk '{print $1}')"
lock_sha256="$(sha256sum Cargo.lock | awk '{print $1}')"
source_commit="$(git rev-parse HEAD 2>/dev/null || printf '%040d' 0)"

jq -S \
    --arg artifact "$(basename "$artifact")" \
    --arg artifact_sha256 "$artifact_sha256" \
    --arg lock_sha256 "$lock_sha256" \
    --arg source_commit "$source_commit" \
    '{
        spdxVersion: "SPDX-2.3",
        dataLicense: "CC0-1.0",
        SPDXID: "SPDXRef-DOCUMENT",
        name: ("aequora-" + $artifact),
        documentNamespace: ("https://aequora.invalid/spdx/" + $source_commit + "/" + $artifact_sha256),
        creationInfo: {
            created: "1970-01-01T00:00:00Z",
            creators: ["Tool: scripts/generate-supply-chain-artifacts.sh"]
        },
        documentDescribes: ["SPDXRef-AequoraArtifact"],
        packages: (
            [{
                name: $artifact,
                SPDXID: "SPDXRef-AequoraArtifact",
                versionInfo: $source_commit,
                downloadLocation: "NOASSERTION",
                filesAnalyzed: false,
                licenseConcluded: "NOASSERTION",
                licenseDeclared: "MIT",
                checksums: [{algorithm: "SHA256", checksumValue: $artifact_sha256}],
                externalRefs: [{
                    referenceCategory: "OTHER",
                    referenceType: "aequora-lockfile-sha256",
                    referenceLocator: $lock_sha256
                }]
            }] + [
                .packages[]
                | {
                    name,
                    SPDXID: ("SPDXRef-Package-" + (.id | gsub("[^A-Za-z0-9.-]"; "-"))),
                    versionInfo: .version,
                    downloadLocation: (.source // "NOASSERTION"),
                    filesAnalyzed: false,
                    licenseConcluded: (.license // "NOASSERTION"),
                    licenseDeclared: (.license // "NOASSERTION"),
                    supplier: (.authors[0] // "NOASSERTION")
                }
            ]
        ),
        relationships: [
            .resolve.nodes[] as $node
            | $node.deps[]
            | {
                spdxElementId: ("SPDXRef-Package-" + ($node.id | gsub("[^A-Za-z0-9.-]"; "-"))),
                relationshipType: "DEPENDS_ON",
                relatedSpdxElement: ("SPDXRef-Package-" + (.pkg | gsub("[^A-Za-z0-9.-]"; "-")))
            }
        ]
    }' "$metadata" >"$output/aequora.spdx.json"

jq -r '
    .packages
    | map(select(.source != null) | [.name, .version, (.license // "UNKNOWN"), .source])
    | unique
    | sort_by(.[0], .[1])
    | .[]
    | @tsv
' "$metadata" >"$output/THIRD_PARTY_LICENSES.txt"

jq -S '{
    resolved_packages: (.packages | length),
    external_packages: ([.packages[] | select(.source != null)] | length),
    duplicate_versions: (
        [.packages[] | select(.source != null) | {name, version}]
        | group_by(.name)
        | map(select(length > 1) | {name: .[0].name, versions: map(.version)})
    ),
    native_boundaries: [
        .packages[]
        | select(.links != null)
        | {name, version, links, source}
    ],
    build_scripts: [
        .packages[]
        | select(any(.targets[]; any(.kind[]; . == "custom-build")))
        | {name, version, source}
    ],
    proc_macros: [
        .packages[]
        | select(any(.targets[]; any(.kind[]; . == "proc-macro")))
        | {name, version, source}
    ],
    sources: [.packages[] | select(.source != null) | {name, version, source}],
    licenses: [.packages[] | select(.source != null) | {name, version, license}]
}' "$metadata" >"$output/dependency-report.json"

{
    echo "# Third-party notices"
    echo
    echo "Generated from the locked Cargo graph for source commit $source_commit."
    echo "Package license metadata is reproduced below for review; applicable upstream license and"
    echo "notice files must accompany the distributed artifact."
    echo
    echo "| Package | Version | Declared license | Source |"
    echo "| --- | --- | --- | --- |"
    jq -r '
        .packages
        | map(select(.source != null) | [.name, .version, (.license // "UNKNOWN"), .source])
        | unique
        | sort_by(.[0], .[1])
        | .[]
        | "| \(.[0]) | \(.[1]) | \(.[2]) | \(.[3]) |"
    ' "$metadata"
} >"$output/THIRD_PARTY_NOTICES.md"

printf 'supply-chain artifacts: %s\n' "$output"
