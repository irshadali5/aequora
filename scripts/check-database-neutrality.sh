#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace_root"

tree_for() {
    local features="$1"
    if [[ -z "$features" ]]; then
        cargo tree -p aequora --no-default-features --edges normal --prefix none --locked
    else
        cargo tree -p aequora --no-default-features --features "$features" \
            --edges normal --prefix none --locked
    fi
}

check_profile() {
    local profile="$1"
    local features="$2"
    if [[ -z "$features" ]]; then
        cargo check -p aequora --lib --no-default-features --locked
    else
        cargo check -p aequora --lib --no-default-features --features "$features" --locked
    fi
    echo "database-neutrality policy: $profile compiles"
}

require_package() {
    local profile="$1"
    local tree="$2"
    local package="$3"
    if ! grep -Eq "^${package} v" <<<"$tree"; then
        echo "database-neutrality policy: $profile must include $package" >&2
        exit 1
    fi
}

forbid_package() {
    local profile="$1"
    local tree="$2"
    local package="$3"
    if grep -Eq "^${package} v" <<<"$tree"; then
        echo "database-neutrality policy: $profile unexpectedly includes $package" >&2
        exit 1
    fi
}

check_profile "custom client + custom authority" ""
neutral_tree="$(tree_for "")"
forbid_package "custom client + custom authority" "$neutral_tree" "stoolap"
forbid_package "custom client + custom authority" "$neutral_tree" "sqlx"
forbid_package "custom client + custom authority" "$neutral_tree" "aequora-store-stoolap"
forbid_package "custom client + custom authority" "$neutral_tree" "aequora-store-postgres"

check_profile "Stoolap client + custom authority" "stoolap,http-client"
client_tree="$(tree_for "stoolap,http-client")"
require_package "Stoolap client + custom authority" "$client_tree" "stoolap"
require_package "Stoolap client + custom authority" "$client_tree" "aequora-store-stoolap"
forbid_package "Stoolap client + custom authority" "$client_tree" "sqlx"
forbid_package "Stoolap client + custom authority" "$client_tree" "aequora-store-postgres"

check_profile "custom client + PostgreSQL authority" "postgres,axum"
authority_tree="$(tree_for "postgres,axum")"
require_package "custom client + PostgreSQL authority" "$authority_tree" "sqlx"
require_package "custom client + PostgreSQL authority" "$authority_tree" "aequora-store-postgres"
forbid_package "custom client + PostgreSQL authority" "$authority_tree" "stoolap"
forbid_package "custom client + PostgreSQL authority" "$authority_tree" "aequora-store-stoolap"

check_profile "Stoolap client + PostgreSQL authority" "stoolap,http-client,postgres,axum"
combined_tree="$(tree_for "stoolap,http-client,postgres,axum")"
require_package "Stoolap client + PostgreSQL authority" "$combined_tree" "stoolap"
require_package "Stoolap client + PostgreSQL authority" "$combined_tree" "sqlx"
require_package "Stoolap client + PostgreSQL authority" "$combined_tree" "aequora-store-stoolap"
require_package "Stoolap client + PostgreSQL authority" "$combined_tree" "aequora-store-postgres"

echo "database-neutrality policy passed for neutral, client-only, authority-only, and combined profiles"
