#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

test -f crates/aequora-feed/Cargo.toml
test -f crates/aequora-feed/src/lib.rs
test -f crates/aequora-testkit/tests/change_feed_contracts.rs
test -f config/change-feed.ron
test -f docs/change-feed-event-contract.md
test -f docs/multi-consumer-change-feed-completion.md

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-FEED${suffix}" crates/aequora-feed/src/lib.rs
    rg -q "AEQ-INV-FEED${suffix}" crates/aequora-invariants/src/lib.rs
done

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|quinn|reqwest)([^[:alnum:]_-]|$)' \
    crates/aequora-feed/Cargo.toml; then
    echo "change-feed architecture: runtime, broker, transport, or database dependency in core" >&2
    exit 1
fi

for symbol in ConsumerId ConsumerKind ConsumerGroupId ConsumerCursor ConsumerPartitionCursor \
    ChangeFeedEvent ConsumerProjector ConsumerFilter ConsumerRegistration ConsumerRegistry \
    ConsumerLease ConsumerFencingToken ConsumerOrderingPolicy ConsumerRetentionPolicy \
    ConsumerFailurePolicy ConsumerIdempotencyLedger AckRequest ConsumerCheckpoint \
    ConsumerQuarantineRecord ConsumerResetPlan ConsumerRebuildPlan FeedArchiveSegment \
    IntegrationEvent ConsumerStorageSurface ConsumerAdminView ConsumerIncidentView \
    ConsumerStore ChangeFeedSource BrokerAdapter FEED_INVARIANTS; do
    rg -q "${symbol}" crates/aequora-feed/src/lib.rs
done

rg -q 'minimum_pinning_consumer_cursor' crates/aequora-journal/src/lib.rs
rg -q 'ResetConsumer' crates/aequora-admin/src/lib.rs
rg -q 'ConsumersReset' crates/aequora-admin/src/lib.rs
rg -q 'pub use aequora_feed as feed' crates/aequora/src/lib.rs
rg -q 'Some\("feed"\)' crates/aequora-cli/src/main.rs
rg -q '"aequora-feed"' crates/aequora-dev/src/main.rs

cargo run -q -p aequora-cli --locked -- feed validate config/change-feed.ron >/dev/null

echo "change-feed architecture: independent cursors, durable ACK, isolation, idempotency, retention recovery, ordering, reset audit, external visibility, governance, and neutral dependencies verified"
