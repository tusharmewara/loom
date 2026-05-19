//! EventBus benchmarks: in-process throughput, latency, backpressure.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use loom_event_bus::{EventBus, InProcessBus, LoomEvent};

fn bench_in_process_throughput(c: &mut Criterion) {
    let bus = InProcessBus::new(65536);
    let event = LoomEvent::SkillsAdded { count: 100 };

    c.bench_function("in_process_publish", |b| {
        b.iter(|| {
            let _ = bus.publish(black_box(&event));
        })
    });
}

fn bench_event_bus_throughput(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let bus = EventBus::with_defaults(tmp.path().to_str().unwrap());
    let _rx = bus.subscribe();
    let event = LoomEvent::SkillsAdded { count: 100 };

    c.bench_function("event_bus_publish_stress", |b| {
        b.iter(|| {
            let _ = bus.publish(black_box(event.clone()));
        })
    });
}

fn bench_in_process_subscriber(c: &mut Criterion) {
    let bus = InProcessBus::new(65536);
    let mut rx = bus.subscribe();
    let event = LoomEvent::McpServerAdded {
        id: "bench-server".into(),
        name: "Benchmark".into(),
    };

    // Warm up
    bus.publish(&event).unwrap();
    let _ = rx.try_recv();

    c.bench_function("in_process_publish_and_recv", |b| {
        b.iter(|| {
            bus.publish(black_box(&event)).unwrap();
            let _ = black_box(rx.try_recv());
        })
    });
}

fn bench_in_process_many_subscribers(c: &mut Criterion) {
    let bus = InProcessBus::new(65536);
    let event = LoomEvent::RoutingUpdated;

    // Subscribe 100 receivers
    let mut rxs: Vec<_> = (0..100).map(|_| bus.subscribe()).collect();

    c.bench_function("in_process_100_subscribers", |b| {
        b.iter(|| {
            bus.publish(black_box(&event)).unwrap();
            // Drain all
            for rx in rxs.iter_mut() {
                let _ = rx.try_recv();
            }
        })
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(100);
    targets = bench_in_process_throughput,
    bench_event_bus_throughput,
    bench_in_process_subscriber,
    bench_in_process_many_subscribers,
);
criterion_main!(benches);
