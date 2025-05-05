use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mavlink_server::callbacks::Callbacks;
use tokio::runtime::Runtime;

fn bench_call_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("callbacks");

    let callback_counts = vec![0, 1, 3, 5, 10, 20, 50, 100];

    for number_of_callbacks in &callback_counts {
        group.throughput(criterion::Throughput::Elements(*number_of_callbacks));
        group.bench_with_input(
            BenchmarkId::from_parameter(number_of_callbacks),
            number_of_callbacks,
            |b, &number_of_callbacks| {
                let rt = Runtime::new().unwrap();
                let callbacks = Callbacks::<String>::default();

                for _ in 0..number_of_callbacks {
                    callbacks.add_callback({
                        move |msg: String| async move {
                            if msg != "test" {
                                panic!("Wrong message");
                            }
                            Ok(())
                        }
                    });
                }

                // Benchmark calling all callbacks
                b.iter(|| {
                    rt.block_on(async {
                        for future in callbacks.call_all("test".to_string()) {
                            if let Err(_error) = future.await {
                                continue;
                            }
                        }
                    });
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_call_all);
criterion_main!(benches);
