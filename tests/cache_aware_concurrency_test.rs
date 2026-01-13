use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use vllm_router_rs::core::{BasicWorker, Worker, WorkerType};
use vllm_router_rs::policies::{CacheAwareConfig, CacheAwarePolicy, LoadBalancingPolicy};

/// Test concurrent access to the cache with multiple threads
/// This test validates that the DashMap-based implementation
/// does not suffer from lock contention
#[test]
fn test_concurrent_cache_operations() {
    let config = CacheAwareConfig {
        cache_threshold: 0.5,
        balance_abs_threshold: 10,
        balance_rel_threshold: 2.0,
        eviction_interval_secs: 0, // Disable eviction thread for testing
        max_tree_size: 10000,
    };

    let policy = Arc::new(CacheAwarePolicy::with_config(config));

    // Create workers
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

    // Number of threads to simulate concurrent requests
    let num_threads = 16;
    let requests_per_thread = 1000;

    let start = Instant::now();

    // Spawn multiple threads that concurrently access the policy
    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let policy = Arc::clone(&policy);
        let workers = Arc::clone(&workers_arc);

        let handle = thread::spawn(move || {
            for i in 0..requests_per_thread {
                // Vary the request text to simulate different cache patterns
                let request_text = format!("request from thread {} iteration {}", thread_id, i);
                
                // Select a worker (this will access the cache)
                let selected = policy.select_worker(&workers, Some(&request_text));
                assert!(selected.is_some(), "Should select a worker");

                // Verify the selection is valid
                let idx = selected.unwrap();
                assert!(idx < workers.len(), "Selected index should be valid");
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread should complete successfully");
    }

    let elapsed = start.elapsed();
    
    println!(
        "Completed {} requests across {} threads in {:?}",
        num_threads * requests_per_thread,
        num_threads,
        elapsed
    );
    
    // With DashMap, the operations should complete quickly
    // Under the old implementation with Mutex, this could take 20+ seconds
    // With DashMap, it should be < 5 seconds even under high load
    assert!(
        elapsed < Duration::from_secs(10),
        "Operations took too long: {:?}, possible lock contention",
        elapsed
    );
}

/// Test concurrent worker additions and removals
#[test]
fn test_concurrent_worker_management() {
    let config = CacheAwareConfig {
        cache_threshold: 0.5,
        balance_abs_threshold: 10,
        balance_rel_threshold: 2.0,
        eviction_interval_secs: 0,
        max_tree_size: 10000,
    };

    let policy = Arc::new(CacheAwarePolicy::with_config(config));

    let num_threads = 8;
    let operations_per_thread = 100;

    let start = Instant::now();

    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let policy = Arc::clone(&policy);

        let handle = thread::spawn(move || {
            for i in 0..operations_per_thread {
                let worker_url = format!("http://worker-{}-{}:8000", thread_id, i);
                let worker = BasicWorker::new(worker_url.clone(), WorkerType::Regular);

                // Add worker
                policy.add_worker(&worker);

                // Remove worker
                policy.remove_worker(&worker);
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread should complete successfully");
    }

    let elapsed = start.elapsed();
    
    println!(
        "Completed {} worker add/remove operations across {} threads in {:?}",
        num_threads * operations_per_thread * 2,
        num_threads,
        elapsed
    );

    // Should complete quickly without lock contention
    assert!(
        elapsed < Duration::from_secs(5),
        "Worker operations took too long: {:?}",
        elapsed
    );
}

/// Test that eviction thread can run concurrently with request processing
#[test]
fn test_concurrent_eviction_and_requests() {
    let config = CacheAwareConfig {
        cache_threshold: 0.5,
        balance_abs_threshold: 10,
        balance_rel_threshold: 2.0,
        eviction_interval_secs: 1, // Enable eviction with 1 second interval
        max_tree_size: 100, // Small size to trigger evictions
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
    ];

    policy.init_workers(&workers);
    let workers_arc = Arc::new(workers);

    // Run requests concurrently while eviction thread is active
    let num_threads = 4;
    let test_duration = Duration::from_secs(3);
    let start = Instant::now();

    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let policy = Arc::clone(&policy);
        let workers = Arc::clone(&workers_arc);

        let handle = thread::spawn(move || {
            let mut count = 0;
            while start.elapsed() < test_duration {
                let request_text = format!("request from thread {} count {}", thread_id, count);
                let selected = policy.select_worker(&workers, Some(&request_text));
                assert!(selected.is_some());
                count += 1;
                thread::sleep(Duration::from_millis(10));
            }
            count
        });

        handles.push(handle);
    }

    // Wait for all threads and collect counts
    let mut total_requests = 0;
    for handle in handles {
        total_requests += handle.join().expect("Thread should complete successfully");
    }

    println!(
        "Processed {} requests in {:?} with concurrent eviction",
        total_requests,
        start.elapsed()
    );

    assert!(
        total_requests > 0,
        "Should have processed some requests"
    );
}

/// Test high QPS scenario similar to the issue description
#[test]
fn test_high_qps_scenario() {
    let config = CacheAwareConfig {
        cache_threshold: 0.5,
        balance_abs_threshold: 10,
        balance_rel_threshold: 2.0,
        eviction_interval_secs: 0,
        max_tree_size: 10000,
    };

    let policy = Arc::new(CacheAwarePolicy::with_config(config));

    // Create 4 workers to simulate a realistic setup
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

    // Simulate QPS of ~16 for 5 seconds
    let qps = 16;
    let test_duration = Duration::from_secs(5);
    let expected_requests = qps * test_duration.as_secs() as usize;

    let start = Instant::now();
    let mut handles = vec![];

    // Spawn threads to generate the target QPS
    for thread_id in 0..qps {
        let policy = Arc::clone(&policy);
        let workers = Arc::clone(&workers_arc);

        let handle = thread::spawn(move || {
            let mut count = 0;
            let mut max_latency = Duration::from_secs(0);
            
            while start.elapsed() < test_duration {
                let op_start = Instant::now();
                
                // Make a request with varying text
                let request_text = format!("request {} from thread {}", count, thread_id);
                let selected = policy.select_worker(&workers, Some(&request_text));
                
                let latency = op_start.elapsed();
                if latency > max_latency {
                    max_latency = latency;
                }
                
                assert!(selected.is_some(), "Should select a worker");
                count += 1;
                
                // Sleep to maintain ~1 QPS per thread
                thread::sleep(Duration::from_millis(1000));
            }
            
            (count, max_latency)
        });

        handles.push(handle);
    }

    // Collect results
    let mut total_requests = 0;
    let mut max_observed_latency = Duration::from_secs(0);
    
    for handle in handles {
        let (count, max_latency) = handle.join().expect("Thread should complete");
        total_requests += count;
        if max_latency > max_observed_latency {
            max_observed_latency = max_latency;
        }
    }

    let elapsed = start.elapsed();
    let actual_qps = total_requests as f64 / elapsed.as_secs_f64();

    println!(
        "Processed {} requests in {:?} (QPS: {:.2})",
        total_requests, elapsed, actual_qps
    );
    println!("Max observed latency: {:?}", max_observed_latency);

    // The main issue was that tree.insert() could be delayed for up to 20 seconds
    // With DashMap, latency should be much lower (< 1 second even under load)
    assert!(
        max_observed_latency < Duration::from_secs(1),
        "Operation latency too high: {:?}, indicates lock contention",
        max_observed_latency
    );

    // Verify we processed a reasonable number of requests
    assert!(
        total_requests >= expected_requests - qps,
        "Should process approximately {} requests, got {}",
        expected_requests,
        total_requests
    );
}
