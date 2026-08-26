use aequora_live::{HintBroker, HintWakeOutcome, HintWakeTracker, SyncHint, SyncHintReason};
use aequora_testkit::live::FakeHintBroker;
use aequora_types::{Sequence, SyncScopeId, TenantId};

#[tokio::test]
async fn faults_only_change_wakeup_latency_not_wake_semantics() {
    let tenant = TenantId::new();
    let scope = SyncScopeId::new();
    let broker = FakeHintBroker::default();
    let tracker = HintWakeTracker::new(tenant, [scope]);
    let hint = |sequence| {
        SyncHint::v1(
            tenant,
            scope,
            Some(Sequence(sequence)),
            SyncHintReason::NewAuthoritativeChange,
        )
    };

    broker.drop_next(1);
    assert!(broker.publish(hint(1)).await.is_ok());
    assert_eq!(broker.queued(), 0);

    broker.duplicate_next(2);
    assert!(broker.publish(hint(2)).await.is_ok());
    let subscription = broker.subscribe().await;
    assert!(subscription.is_ok());
    let mut subscription = subscription.unwrap_or_else(|_| unreachable!());
    assert_eq!(
        subscription
            .next_hint()
            .await
            .and_then(|item| item.ok_or_else(|| aequora_live::LiveError::new(
                aequora_live::LiveFailureKind::Disconnected,
                "closed"
            )))
            .map(|item| tracker.observe(item)),
        Ok(HintWakeOutcome::Wake { generation: 1 })
    );
    for _ in 0..2 {
        let next = subscription.next_hint().await;
        assert_eq!(
            next.map(|item| item.map(|value| tracker.observe(value))),
            Ok(Some(HintWakeOutcome::Coalesced { generation: 1 }))
        );
    }

    broker.reorder_next_pair();
    assert!(broker.publish(hint(3)).await.is_ok());
    assert!(broker.publish(hint(4)).await.is_ok());
    let first = subscription.next_hint().await;
    let second = subscription.next_hint().await;
    assert_eq!(
        first.map(|item| item.map(|value| value.latest_sequence)),
        Ok(Some(Some(Sequence(4))))
    );
    assert_eq!(
        second.map(|item| item.map(|value| value.latest_sequence)),
        Ok(Some(Some(Sequence(3))))
    );

    broker.delay_next(1);
    assert!(broker.publish(hint(5)).await.is_ok());
    assert_eq!(broker.queued(), 0);
    broker.release_delayed();
    assert_eq!(broker.queued(), 1);
    broker.disconnect();
    assert_eq!(subscription.next_hint().await, Ok(None));
}
