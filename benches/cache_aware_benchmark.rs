use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::thread;
use vllm_router_rs::core::{BasicWorker, Worker, WorkerType};
use vllm_router_rs::policies::{CacheAwareConfig, CacheAwarePolicy, LoadBalancingPolicy};

/// Benchmark cache-aware policy under concurrent load
fn bench_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_aware_concurrent");

    // Test with different thread counts
    for num_threads in [1, 4, 8, 16].iter() {
        group.throughput(Throughput::Elements(*num_threads as u64 * 100));
        
        group.bench_with_input(
            BenchmarkId::new("select_worker", num_threads),
            num_threads,
            |b, &num_threads| {
                let config = CacheAwareConfig {
                    cache_threshold: 0.5,
                    balance_abs_threshold: 10,
                    balance_rel_threshold: 2.0,
                    eviction_interval_secs: 0,
                    max_tree_size: 10000,
                };

                let policy = Arc::new(CacheAwarePolicy::with_config(config));

                let workers: Vec<Arc<dyn Worker>> = vec![
                    Arc::new(BasicWorker::new(
                        "http://w1:8000".to_string(),
                        WorkerType::Regular,
                    )),
                    Arc::new(BasicWorker::new(
                        "http://w2:8000".to_string(),
                        WorkerType::Regular,
                    )),
                    Arc::new(BasicWorker::new(
                        "http://w3:8000".to_string(),
                        WorkerType::Regular,
                    )),
                    Arc::new(BasicWorker::new(
                        "http://w4:8000".to_string(),
                        WorkerType::Regular,
                    )),
                ];

                policy.init_workers(&workers);
                let workers_arc = Arc::new(workers);

                b.iter(|| {
                    let mut handles = vec![];
                    
                    for thread_id in 0..num_threads {
                        let policy = Arc::clone(&policy);
                        let workers = Arc::clone(&workers_arc);

                        let handle = thread::spawn(move || {
                            for i in 0..100 {
                                let request_text = format!("request {} from thread {}", i, thread_id);
                                let _ = black_box(policy.select_worker(&workers, Some(&request_text)));
                            }
                        });

                        handles.push(handle);
                    }

                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark worker management operations
fn bench_worker_management(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_aware_worker_mgmt");

    group.bench_function("add_remove_worker", |b| {
        let config = CacheAwareConfig::default();
        let policy = CacheAwarePolicy::with_config(config);

        b.iter(|| {
            for i in 0..100 {
                let worker_url = format!("http://worker-{}:8000", i);
                let worker = BasicWorker::new(worker_url.clone(), WorkerType::Regular);
                
                policy.add_worker(&worker);
                policy.remove_worker(&worker);
            }
        });
    });

    group.finish();
}

/// Benchmark single-threaded performance
fn bench_single_thread(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_aware_single_thread");

    group.bench_function("select_worker_100_requests", |b| {
        let config = CacheAwareConfig {
            cache_threshold: 0.5,
            balance_abs_threshold: 10,
            balance_rel_threshold: 2.0,
            eviction_interval_secs: 0,
            max_tree_size: 10000,
        };

        let policy = CacheAwarePolicy::with_config(config);

        let workers: Vec<Arc<dyn Worker>> = vec![
            Arc::new(BasicWorker::new(
                "http://w1:8000".to_string(),
                WorkerType::Regular,
            )),
            Arc::new(BasicWorker::new(
                "http://w2:8000".to_string(),
                WorkerType::Regular,
            )),
        ];

        policy.init_workers(&workers);

        b.iter(|| {
            for i in 0..100 {
                let request_text = format!("request {}", i);
                let _ = black_box(policy.select_worker(&workers, Some(&request_text)));
            }
        });
    });

    group.finish();
}

/// Benchmark cache hit scenarios
fn bench_cache_hits(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_aware_cache_hits");

    group.bench_function("repeated_same_request", |b| {
        let config = CacheAwareConfig {
            cache_threshold: 0.5,
            balance_abs_threshold: 10,
            balance_rel_threshold: 2.0,
            eviction_interval_secs: 0,
            max_tree_size: 10000,
        };

        let policy = CacheAwarePolicy::with_config(config);

        let workers: Vec<Arc<dyn Worker>> = vec![
            Arc::new(BasicWorker::new(
                "http://w1:8000".to_string(),
                WorkerType::Regular,
            )),
            Arc::new(BasicWorker::new(
                "http://w2:8000".to_string(),
                WorkerType::Regular,
            )),
        ];

        policy.init_workers(&workers);

        // Warmup with same request
        let request_text = "This is a test request that will be repeated";
        policy.select_worker(&workers, Some(request_text));

        b.iter(|| {
            for _ in 0..100 {
                let _ = black_box(policy.select_worker(&workers, Some(request_text)));
            }
        });
    });

    group.finish();
}

/// Benchmark high QPS scenario (main use case from the issue)
fn bench_high_qps(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_aware_high_qps");
    group.sample_size(10); // Reduce sample size for this intensive test

    group.bench_function("qps_16_threads_1000_requests", |b| {
        let config = CacheAwareConfig {
            cache_threshold: 0.5,
            balance_abs_threshold: 10,
            balance_rel_threshold: 2.0,
            eviction_interval_secs: 0,
            max_tree_size: 10000,
        };

        let policy = Arc::new(CacheAwarePolicy::with_config(config));

        let workers: Vec<Arc<dyn Worker>> = vec![
            Arc::new(BasicWorker::new(
                "http://w1:8000".to_string(),
                WorkerType::Regular,
            )),
            Arc::new(BasicWorker::new(
                "http://w2:8000".to_string(),
                WorkerType::Regular,
            )),
            Arc::new(BasicWorker::new(
                "http://w3:8000".to_string(),
                WorkerType::Regular,
            )),
            Arc::new(BasicWorker::new(
                "http://w4:8000".to_string(),
                WorkerType::Regular,
            )),
        ];

        policy.init_workers(&workers);
        let workers_arc = Arc::new(workers);

        b.iter(|| {
            let mut handles = vec![];
            
            for thread_id in 0..16 {
                let policy = Arc::clone(&policy);
                let workers = Arc::clone(&workers_arc);

                let handle = thread::spawn(move || {
                    for i in 0..1000 {
                        let request_text = format!("request {} from thread {}", i, thread_id);
                        let _ = black_box(policy.select_worker(&workers, Some(&request_text)));
                    }
                });

                handles.push(handle);
            }

            for handle in handles {
                handle.join().unwrap();
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_concurrent_operations,
    bench_worker_management,
    bench_single_thread,
    bench_cache_hits,
    bench_high_qps,
);
criterion_main!(benches);
