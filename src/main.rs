use clap::clap_app;
use crossbeam::channel::{Receiver, Sender};
use leaky_bucket::LeakyBucket;
use matplotrust::{histogram, line_plot, Figure};
use std::collections::HashMap;
use std::ops::AddAssign;
use std::thread;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tokio::time::delay_for;

const TIMEOUT: u64 = 1_000;

#[derive(Clone)]
struct Task {
    start: Instant,
    cost: u64,
}

#[derive(Clone)]
struct TaskStats {
    success: bool,
    start_time: Instant,
    completion_time: Instant,
    overhead: f64,
}

#[derive(Debug)]
enum Mode {
    Sync(usize),
    Async,
}

#[derive(Debug)]
struct ModelConfig {
    name: String,
    n_jobs: usize,
    rps: usize,
    latency_distribution: Vec<u64>,
    python_path: Option<String>,
    mode: Mode,
}

#[tokio::main]
async fn main() {
    let config = ModelConfig::from_cli();
    println!("Config: {:#?}", config);

    let mut duration_ms = 1000;
    let mut refill = config.rps;
    while duration_ms > 1 && refill % 10 == 0 {
        duration_ms /= 10;
        refill /= 10;
    }
    println!("Rate limit refill {} per {}ms", refill, duration_ms);
    let rate_limiter = LeakyBucket::builder()
        .refill_amount(refill)
        .refill_interval(Duration::from_millis(duration_ms as u64))
        .build()
        .expect("LeakyBucket builder failed");

    let start_time = Instant::now();

    let (latencies, rps_buckets) = match config.mode {
        Mode::Sync(n_workers) => {
            let (stats_store, stats_recv) = crossbeam::channel::bounded::<TaskStats>(config.n_jobs);
            sync_execution(
                n_workers,
                &config.latency_distribution,
                config.n_jobs,
                rate_limiter,
                stats_store.clone(),
            )
            .await;
            process_sync_stats(start_time, stats_recv)
        }
        Mode::Async => {
            let (stats_store, stats_recv) = tokio::sync::mpsc::channel::<TaskStats>(config.n_jobs);
            async_execution(
                &config.latency_distribution,
                config.n_jobs,
                rate_limiter,
                stats_store.clone(),
            )
            .await;
            process_async_stats(config.n_jobs, start_time, stats_recv).await
        }
    };

    build_latency_timeline(&config, latencies.clone());
    build_latency_histogram(&config, latencies);
    build_rps_graph(&config, rps_buckets);
}

fn process_sync_stats(
    start_time: Instant,
    stats_recv: Receiver<TaskStats>,
) -> (Vec<TaskStats>, HashMap<u64, u64>) {
    let mut latencies = vec![];
    let mut rps_buckets = HashMap::new();
    while !stats_recv.is_empty() {
        let stats = stats_recv.recv().expect("Must has an element");
        if stats.success {
            latencies.push(stats.clone());
            rps_buckets
                .entry(stats.completion_time.duration_since(start_time).as_secs())
                .or_insert(0)
                .add_assign(1);
        }
    }
    (latencies, rps_buckets)
}

async fn process_async_stats(
    n_jobs: usize,
    start_time: Instant,
    mut stats_recv: tokio::sync::mpsc::Receiver<TaskStats>,
) -> (Vec<TaskStats>, HashMap<u64, u64>) {
    let mut latencies = vec![];
    let mut rps_buckets = HashMap::new();
    for _ in 0..n_jobs {
        let stats = stats_recv.recv().await.expect("Must has an element");
        if stats.success {
            latencies.push(stats.clone());
            rps_buckets
                .entry(stats.completion_time.duration_since(start_time).as_secs())
                .or_insert(0)
                .add_assign(1);
        }
    }
    (latencies, rps_buckets)
}

impl ModelConfig {
    fn from_cli() -> Self {
        let matches = clap_app!(myapp =>
            (name: "Model Sync/Async execution")
            (version: "0.0.1")
            (author: "Eugene Retunsky")
            (about: "A tool to model sync vs async processing for a network service")
            (@arg NAME: --name -N +takes_value +required "Name of the test-case")
            (@arg RATE: --rate -r +takes_value +required "Request rate per second. E.g. 100 or 1000")
            (@arg NUM_REQUESTS: --num_req -n +takes_value +required "Number of requests. E.g. 1000")
            (@arg LATENCY_DISTRIBUTION: --latency -l +takes_value +required "Comma separated latency values. E.g. 200,200,200,500")
            (@arg PYTHON_PATH: --python_path -p +takes_value "Optional path to python3, e.g. /usr/bin/python3")
            (@subcommand async =>
                (about: "Model a service with Async I/O")
                (version: "0.0.1")
            )
            (@subcommand sync =>
                (about: "Model a service with Blocking I/O")
                (version: "0.0.1")
                (@arg THREADS: --threads -t +takes_value +required "The number of worker threads")
            )
        ).get_matches();

        Self {
            name: matches
                .value_of("NAME")
                .expect("Name is required")
                .to_string(),
            n_jobs: matches
                .value_of("NUM_REQUESTS")
                .expect("Rate is required")
                .parse()
                .expect("NUM_REQUESTS must be a positive integer"),
            rps: matches
                .value_of("RATE")
                .expect("Rate is required")
                .parse()
                .expect("RATE must be a positive integer"),
            latency_distribution: matches
                .value_of("LATENCY_DISTRIBUTION")
                .expect("Rate is required")
                .split(',')
                .map(|s| ModelConfig::parse_latency_item(s))
                .flatten()
                .collect(),
            python_path: matches.value_of("PYTHON_PATH").map(|s| s.to_string()),
            mode: if let Some(config) = matches.subcommand_matches("sync") {
                Mode::Sync(
                    config
                        .value_of("THREADS")
                        .expect("Rate is required")
                        .parse()
                        .expect("THREADS must be a positive integer"),
                )
            } else {
                Mode::Async
            },
        }
    }

    fn parse_latency_item(s: &str) -> Vec<u64> {
        if !s.contains('*') {
            vec![s.parse().expect("Latency items must be positive numbers")]
        } else {
            let mut split = s.split('*');
            let value = split.next().expect("Must be in format `value*count`");
            let count: usize = split
                .next()
                .expect("Must be in format `value*count`")
                .parse()
                .expect("Illegal numeric value");
            (0..count)
                .map(|_| value.parse().expect("Illegal numeric value"))
                .collect()
        }
    }

    fn get_python_path(&self) -> Option<&str> {
        let python_path = match self.python_path.as_ref() {
            None => None,
            Some(s) => Some(s.as_str()),
        };
        python_path
    }
}

async fn sync_execution(
    n_workers: usize,
    latency_distribution: &[u64],
    n_jobs: usize,
    rate_limiter: LeakyBucket,
    stats_store: Sender<TaskStats>,
) {
    let mut threads = Vec::with_capacity(n_workers);
    let (send, recv) = crossbeam::channel::bounded::<Task>(n_jobs);

    for _ in 0..n_workers {
        let receiver = recv.clone();
        let stats_sender = stats_store.clone();

        threads.push(thread::spawn(move || {
            for val in receiver {
                sleep(Duration::from_millis(val.cost));
                // report metrics
                let now = Instant::now();
                let stats = TaskStats {
                    start_time: val.start,
                    success: val.cost < TIMEOUT,
                    completion_time: now,
                    overhead: now.duration_since(val.start).as_secs_f64() - val.cost as f64 / 1000.,
                };
                stats_sender.try_send(stats).unwrap_or_default();
            }
        }));
    }

    let start = Instant::now();
    println!("Starting sending tasks...");

    for i in 0..n_jobs {
        rate_limiter.acquire_one().await.unwrap_or_default();
        let cost = latency_distribution[i % latency_distribution.len()];
        let now = Instant::now();
        send.try_send(Task { start: now, cost }).unwrap();
    }

    println!("Waiting for completion...");

    while stats_store.len() < n_jobs as usize {
        sleep(Duration::from_secs(1));
    }

    let elapsed = Instant::now().duration_since(start);
    println!(
        "Elapsed {:?}, rate: {:.3} tasks per second",
        elapsed,
        n_jobs as f64 / elapsed.as_secs_f64()
    );

    drop(send);

    for t in threads {
        t.join().unwrap();
    }
}

async fn async_execution(
    latency_distribution: &[u64],
    n_jobs: usize,
    rate_limiter: LeakyBucket,
    stats_store: tokio::sync::mpsc::Sender<TaskStats>,
) {
    let mut tasks = Vec::with_capacity(n_jobs);

    let start = Instant::now();
    println!("Starting sending tasks...");

    for i in 0..n_jobs {
        rate_limiter.acquire_one().await.unwrap_or_default();
        let cost = latency_distribution[i % latency_distribution.len()];
        let mut stats_sender = stats_store.clone();
        let start = Instant::now();
        tasks.push(tokio::spawn(async move {
            delay_for(Duration::from_millis(cost)).await;

            let now = Instant::now();
            let stats = TaskStats {
                start_time: start,
                success: cost < 1_000,
                completion_time: now,
                overhead: now.duration_since(start).as_secs_f64() - cost as f64 / 1000.,
            };
            stats_sender.send(stats).await.unwrap_or_default();
        }));
    }

    println!("Waiting for completion...");

    for t in tasks {
        t.await.unwrap();
    }

    let elapsed = Instant::now().duration_since(start);
    println!(
        "Elapsed {:?}, rate: {:.3} tasks per second",
        elapsed,
        n_jobs as f64 / elapsed.as_secs_f64()
    );
}

fn build_rps_graph(config: &ModelConfig, rps_buckets: HashMap<u64, u64>) {
    // ignore the first and the last second as they may be incomplete
    let start = 1
        + rps_buckets
            .iter()
            .map(|(k, _)| k)
            .min()
            .expect("At least single data point must be here")
        + 1;
    let end = rps_buckets
        .iter()
        .map(|(k, _)| k)
        .max()
        .expect("At least single data point must be here")
        - 1;
    let mut x = vec![0];
    let mut y = vec![0];
    let mut total = 0.;
    for i in start..end {
        let value = *rps_buckets.get(&i).unwrap_or(&0);
        let time_since_start = i - start;
        x.push(time_since_start);
        y.push(value);
        total += value as f64;
    }

    let avg = total / (end - start) as f64;
    let mut deviation = 0.;
    for i in start..end {
        let value = *rps_buckets.get(&i).unwrap_or(&0);
        deviation += (avg - value as f64) * (avg - value as f64);
    }

    println!("Avg rate: {:.3}, StdDev: {:.3}", avg, deviation.sqrt());

    let line_plot = line_plot::<u64, u64>(x, y, None);
    let mut figure = Figure::new();
    figure.add_plot(line_plot.clone());
    figure.add_plot(line_plot);
    figure.save(
        format!("./figures/request_rate_{}.png", config.name).as_str(),
        config.get_python_path(),
    );
}

fn build_latency_histogram(config: &ModelConfig, mut latencies: Vec<TaskStats>) {
    println!("Latencies:");

    latencies.sort_by(|a, b| a.overhead.partial_cmp(&b.overhead).unwrap());
    let mut percentiles_x = vec![];
    let mut percentiles_y = vec![];
    let printed_percentiles = vec![0, 5000, 9000, 9500, 9900, 9990, 9999, 10000];

    for p in 0..=10000 {
        let stats =
            &latencies[((p as f64 / 10000. * latencies.len() as f64) as i32 - 1).max(0) as usize];
        let value = stats.overhead;
        if printed_percentiles.contains(&p) {
            println!(
                "{} - {}",
                format!("p{:.3}", p as f64 / 100.),
                format!("{:.3} ms", value * 1000.),
            );
        }
        percentiles_x.push(p as f64 / 100.);
        percentiles_y.push(value * 1000.);
    }

    let mut figure = Figure::new();
    let x = latencies.iter().map(|v| v.overhead * 1000.).collect();
    let plot = histogram::<f64>(x, None);
    figure.add_plot(plot);

    figure.save(
        format!("./figures/latency_histogram_{}.png", config.name).as_str(),
        config.get_python_path(),
    );

    let line_plot = line_plot::<f64, f64>(percentiles_x, percentiles_y, None);
    let mut figure = Figure::new();
    figure.add_plot(line_plot.clone());
    figure.add_plot(line_plot);
    figure.save(
        format!("./figures/latency_percentiles_{}.png", config.name).as_str(),
        config.get_python_path(),
    );
}

fn build_latency_timeline(config: &ModelConfig, mut latencies: Vec<TaskStats>) {
    println!("Latencies:");

    latencies.sort_by(|a, b| a.start_time.partial_cmp(&b.start_time).unwrap());

    let mut timeline_x = vec![];
    let mut p50_y = vec![];
    let mut p90_y = vec![];
    let mut p99_y = vec![];

    let mut start = latencies[0].start_time;
    let mut current_x = 0;
    let mut next_second_latency_batch: Vec<f64> = vec![];

    for (i, task) in latencies.iter().enumerate() {
        let moment = task.start_time;
        if moment.duration_since(start).as_secs_f64() >= 1. || i == latencies.len() - 1 {
            timeline_x.push(current_x);
            current_x += 1;

            next_second_latency_batch.sort_by(|a, b| a.partial_cmp(&b).unwrap());
            let batch_size = next_second_latency_batch.len();
            p50_y.push(next_second_latency_batch[batch_size / 2 - 1] * 1000.);
            p90_y.push(next_second_latency_batch[batch_size * 9 / 10 - 1] * 1000.);
            p99_y.push(next_second_latency_batch[batch_size * 99 / 100 - 1] * 1000.);

            start = moment;
        } else {
            next_second_latency_batch.push(task.overhead);
        }
    }

    let mut figure = Figure::new();
    let p50_plot = line_plot::<u64, f64>(timeline_x.clone(), p50_y, None);
    let p90_plot = line_plot::<u64, f64>(timeline_x.clone(), p90_y, None);
    let p99_plot = line_plot::<u64, f64>(timeline_x, p99_y, None);
    figure.add_plot(p50_plot);
    figure.add_plot(p90_plot);
    figure.add_plot(p99_plot);
    figure.save(
        format!("./figures/latency_timeline_{}.png", config.name).as_str(),
        config.get_python_path(),
    );
}
