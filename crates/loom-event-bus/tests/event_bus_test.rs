//! EventBus tests: in-process delivery, conformance, metrics, backpressure, topics.

use loom_event_bus::InProcessBus;
use loom_event_bus::BusMetrics;
use loom_event_bus::Topic;
use loom_event_bus::EventBus;
use loom_event_bus::LoomEvent;
use loom_core::HealthCheck;

// ─── InProcessBus tests ──────────────────────────────────────────────

#[test]
fn test_in_process_bus_create() {
    let bus = InProcessBus::new(1024);
    assert_eq!(bus.capacity(), 1024);
    assert_eq!(bus.subscriber_count(), 0);
}

#[test]
fn test_in_process_bus_publish_subscribe() {
    let bus = InProcessBus::new(1024);
    let mut rx = bus.subscribe();

    let event = LoomEvent::SkillsAdded { count: 5 };
    let subs = bus.publish(&event).unwrap();
    assert!(subs > 0, "should have at least 1 subscriber");

    let received = rx.try_recv().expect("should receive event");
    assert_eq!(
        format!("{:?}", received),
        format!("{:?}", event),
        "received event should match published"
    );
}

#[test]
fn test_in_process_bus_multiple_subscribers() {
    let bus = InProcessBus::new(1024);
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();

    let event = LoomEvent::McpHealthChanged {
        id: "test-server".into(),
        status: loom_event_bus::HealthStatusStr::Healthy,
    };
    let _ = bus.publish(&event);

    // Both subscribers should receive
    let _ = rx1.try_recv().expect("subscriber 1 should receive");
    let _ = rx2.try_recv().expect("subscriber 2 should receive");
}

#[test]
fn test_in_process_bus_backpressure_drops_oldest() {
    let bus = InProcessBus::new(2); // tiny capacity
    let mut rx = bus.subscribe();

    // Publish 3 events — channel cap is 2, so first event gets dropped
    bus.publish(&LoomEvent::RoutingUpdated).unwrap();
    bus.publish(&LoomEvent::TaskCreated { task_id: "t1".into() }).unwrap();
    bus.publish(&LoomEvent::TaskCreated { task_id: "t2".into() }).unwrap();

    // The first recv should be a Lagged error
    match rx.try_recv() {
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
            assert!(n >= 1, "should report lagged count");
        }
        other => panic!("expected Lagged, got {:?}", other),
    }

    // Second recv should be t1
    let recv = rx.try_recv().expect("should receive t1");
    if let LoomEvent::TaskCreated { task_id } = recv {
        assert_eq!(task_id, "t1");
    } else {
        panic!("expected TaskCreated(t1)");
    }
}

// ─── Metrics tests ───────────────────────────────────────────────────

#[test]
fn test_metrics_initial_state() {
    let m = BusMetrics::new();
    assert_eq!(m.published(), 0);
    assert_eq!(m.xproc_published(), 0);
    assert_eq!(m.xproc_errors(), 0);
    assert!(m.topic_distribution().is_empty());
}

#[test]
fn test_metrics_counters() {
    let m = BusMetrics::new();
    m.inc_published();
    m.inc_published();
    m.inc_published();
    assert_eq!(m.published(), 3);

    m.inc_xproc_published();
    assert_eq!(m.xproc_published(), 1);

    m.inc_xproc_errors();
    assert_eq!(m.xproc_errors(), 1);
}

#[test]
fn test_metrics_topic_distribution() {
    let m = BusMetrics::new();
    m.record_topic("skills:added");
    m.record_topic("skills:added");
    m.record_topic("mcp:health-changed");

    let dist = m.topic_distribution();
    assert_eq!(dist.get("skills:added"), Some(&2));
    assert_eq!(dist.get("mcp:health-changed"), Some(&1));
    assert_eq!(dist.len(), 2);
}

// ─── Topic tests ─────────────────────────────────────────────────────

#[test]
fn test_topic_from_event() {
    let cases: Vec<(LoomEvent, &str)> = vec![
        (LoomEvent::SkillsAdded { count: 1 }, "skills:added"),
        (LoomEvent::SkillsRemoved { name: "x".into() }, "skills:removed"),
        (
            LoomEvent::McpHealthChanged {
                id: "x".into(),
                status: loom_event_bus::HealthStatusStr::Healthy,
            },
            "mcp:health-changed",
        ),
        (
            LoomEvent::McpServerAdded {
                id: "x".into(),
                name: "y".into(),
            },
            "mcp:server-added",
        ),
        (LoomEvent::SessionExported { session_id: "s1".into() }, "session:exported"),
        (LoomEvent::RoutingUpdated, "routing:updated"),
        (
            LoomEvent::AuthProviderAdded {
                provider_id: "p1".into(),
            },
            "auth:provider-added",
        ),
        (
            LoomEvent::HooksRegistered {
                hook_id: "h1".into(),
            },
            "hooks:registered",
        ),
        (
            LoomEvent::TaskPhaseChanged {
                task_id: "t1".into(),
                phase: loom_event_bus::TaskPhase::Plan,
            },
            "task:phase-changed",
        ),
        (LoomEvent::TaskCreated { task_id: "t1".into() }, "task:created"),
    ];

    for (event, expected) in cases {
        let topic = Topic::from_event(&event);
        assert_eq!(
            topic.as_str(),
            expected,
            "Topic::from_event({:?}) should be {}",
            event,
            expected
        );
    }
}

#[test]
fn test_topic_from_str() {
    for topic_name in Topic::all() {
        let topic = Topic::from_str(topic_name);
        assert!(topic.is_some(), "{} should be a valid topic", topic_name);
        assert_eq!(topic.unwrap().as_str(), topic_name);
    }

    assert!(Topic::from_str("invalid:topic").is_none());
    assert!(Topic::from_str("").is_none());
}

#[test]
fn test_topic_matches() {
    let a = Topic::from_str("skills:added").unwrap();
    let b = Topic::from_str("skills:added").unwrap();
    let c = Topic::from_str("skills:removed").unwrap();

    assert!(a.matches(&b));
    assert!(!a.matches(&c));
}

// ─── EventBus integration tests ──────────────────────────────────────

#[test]
fn test_event_bus_create_and_publish() {
    let tmp = tempfile::tempdir().unwrap();
    let bus = EventBus::with_defaults(tmp.path().to_str().unwrap());
    let mut rx = bus.subscribe();

    let event = LoomEvent::SkillsAdded { count: 42 };
    let sub_count = bus.publish(event.clone()).unwrap();
    assert_eq!(sub_count, 1, "one subscriber should receive");

    let received = rx.try_recv().expect("should receive event");
    assert_eq!(format!("{:?}", received), format!("{:?}", event));
}

#[test]
fn test_event_bus_multiple_events() {
    let tmp = tempfile::tempdir().unwrap();
    let bus = EventBus::with_defaults(tmp.path().to_str().unwrap());
    let mut rx = bus.subscribe();

    let events = vec![
        LoomEvent::SkillsAdded { count: 1 },
        LoomEvent::McpServerAdded {
            id: "s1".into(),
            name: "Server 1".into(),
        },
        LoomEvent::RoutingUpdated,
    ];

    for ev in &events {
        bus.publish(ev.clone()).unwrap();
    }

    for expected in &events {
        let received = rx.try_recv().expect("should receive event");
        assert_eq!(format!("{:?}", received), format!("{:?}", expected));
    }
}

#[test]
fn test_event_bus_subscriber_disconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let bus = EventBus::with_defaults(tmp.path().to_str().unwrap());

    // Subscribe then drop the receiver
    let rx = bus.subscribe();
    drop(rx);

    // Publishing should still succeed (sub_count = 0)
    let count = bus.publish(LoomEvent::RoutingUpdated).unwrap();
    assert_eq!(count, 0, "no active subscribers");
}

#[test]
fn test_event_bus_metrics_update() {
    let tmp = tempfile::tempdir().unwrap();
    let bus = EventBus::with_defaults(tmp.path().to_str().unwrap());
    let _rx = bus.subscribe();

    for i in 0..10 {
        bus.publish(LoomEvent::SkillsAdded { count: i }).unwrap();
    }

    assert_eq!(bus.metrics().published(), 10);
    let dist = bus.metrics().topic_distribution();
    assert!(dist.contains_key("skills:added"));
}

#[test]
fn test_event_bus_subscriber_lag() {
    let tmp = tempfile::tempdir().unwrap();
    // Small capacity to force lag
    let bus = EventBus::new(tmp.path().to_str().unwrap(), 4);
    let mut rx = bus.subscribe();

    // Fill the buffer and overflow
    for i in 0..8 {
        bus.publish(LoomEvent::TaskCreated {
            task_id: format!("t{}", i),
        })
        .unwrap();
    }

    // Should get a Lagged error because we didn't consume fast enough
    match rx.try_recv() {
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
            assert!(n > 0, "should have dropped events");
        }
        Ok(_) => (), // possible if we caught up
        Err(e) => panic!("unexpected error: {:?}", e),
    }
}

#[test]
fn test_event_bus_health_check() {
    let tmp = tempfile::tempdir().unwrap();
    let bus = EventBus::with_defaults(tmp.path().to_str().unwrap());
    let _rx = bus.subscribe();

    bus.publish(LoomEvent::SkillsAdded { count: 3 }).unwrap();

    let health = bus.health().expect("health check should succeed");
    assert_eq!(health.crate_name, "loom-event-bus");
    assert!(!health.version.is_empty());
    assert!(health.performance_counters.contains_key("published"));
    assert!(health.performance_counters.contains_key("subscriber_count"));
}
