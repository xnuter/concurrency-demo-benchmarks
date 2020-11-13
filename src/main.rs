use clap::clap_app;
use humantime::parse_duration;
use leaky_bucket::LeakyBucket;
use matplotrust::{histogram, line_plot, Figure};
use std::collections::HashMap;
use std::ops::AddAssign;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tokio::time::delay_for;

const TIMEOUT: Duration = Duration::from_secs(1);

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
    while duration_ms > 10 && refill % 10 == 0 {
        duration_ms /= 10;
        refill /= 10;
    }
    println!("Rate limit refill {} per {} ms", refill, duration_ms);
    let rate_limiter = LeakyBucket::builder()
        .refill_amount(refill)
        .refill_interval(Duration::from_millis(duration_ms as u64))
        .build()
        .expect("LeakyBucket builder failed");

    let start_time = Instant::now();

    let stats = match config.mode {
        Mode::Sync(n_workers) => {
            sync_execution(
                n_workers,
                &config.latency_distribution,
                config.n_jobs,
                rate_limiter,
            )
            .await
        }
        Mode::Async => {
            async_execution(&config.latency_distribution, config.n_jobs, rate_limiter).await
        }
    };

    let (latencies, rps_buckets) = process_stats(start_time, stats);

    build_latency_timeline(&config, latencies.clone());
    build_latency_histogram(&config, latencies);
    build_rps_graph(&config, rps_buckets);
}

/// Model multi-thread environment, where each threads can handle
/// a single connection at a time.
async fn sync_execution(
    n_workers: usize,
    latency_distribution: &[u64],
    n_jobs: usize,
    rate_limiter: LeakyBucket,
) -> Vec<TaskStats> {
    let mut threads = Vec::with_capacity(n_workers);
    let (send, recv) = crossbeam::channel::bounded::<Task>(n_jobs);
    static TASK_COUNTER: AtomicUsize = AtomicUsize::new(0);

    for _ in 0..n_workers {
        let receiver = recv.clone();

        threads.push(thread::spawn(move || {
            let mut thread_stats = vec![];
            for val in receiver {
                sleep(Duration::from_millis(val.cost));
                // report metrics
                let now = Instant::now();
                let stats = TaskStats {
                    start_time: val.start,
                    success: val.cost < TIMEOUT.as_millis() as u64,
                    completion_time: now,
                    overhead: now.duration_since(val.start).as_secs_f64() - val.cost as f64 / 1000.,
                };
                thread_stats.push(stats);
                TASK_COUNTER.fetch_add(1, Ordering::Relaxed);
            }
            thread_stats
        }));
    }

    println!("Starting sending tasks...");

    for i in 0..n_jobs {
        rate_limiter.acquire_one().await.unwrap_or_default();
        let cost = latency_distribution[i % latency_distribution.len()];
        let now = Instant::now();
        send.send(Task { start: now, cost }).unwrap();
    }

    println!("Waiting for completion...");

    while TASK_COUNTER.load(Ordering::Relaxed) < n_jobs {
        sleep(Duration::from_secs(1));
    }

    drop(send);

    let mut combined_stats = vec![];
    for t in threads {
        let thread_stats = t.join().unwrap();
        combined_stats.extend(thread_stats);
    }

    combined_stats
}

/// Model an async environment, where there are several threads
/// handling up to tens (or hundreds) of thousands of connections simultaneously.
async fn async_execution(
    latency_distribution: &[u64],
    n_jobs: usize,
    rate_limiter: LeakyBucket,
) -> Vec<TaskStats> {
    let mut tasks = Vec::with_capacity(n_jobs);

    println!("Starting sending tasks...");

    for i in 0..n_jobs {
        rate_limiter.acquire_one().await.unwrap_or_default();
        let cost = latency_distribution[i % latency_distribution.len()];
        let start = Instant::now();
        tasks.push(tokio::spawn(async move {
            delay_for(Duration::from_millis(cost)).await;

            let now = Instant::now();
            TaskStats {
                start_time: start,
                success: cost < TIMEOUT.as_millis() as u64,
                completion_time: now,
                overhead: now.duration_since(start).as_secs_f64() - cost as f64 / 1000.,
            }
        }));
    }

    println!("Waiting for completion...");

    let mut combined_stats = vec![];
    for t in tasks {
        combined_stats.push(t.await.expect("Task failed"));
    }

    combined_stats
}

fn process_stats(
    start_time: Instant,
    stats_collection: Vec<TaskStats>,
) -> (Vec<TaskStats>, HashMap<u64, u64>) {
    let mut latencies = vec![];
    let mut rps_buckets = HashMap::new();
    for stats in stats_collection {
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
            vec![ModelConfig::parse_latency(s)]
        } else {
            let mut split = s.split('*');
            let value = split.next().expect("Must be in format `value*count`");
            let count: usize = split
                .next()
                .expect("Must be in format `value*count`")
                .parse()
                .expect("Illegal numeric value");
            (0..count)
                .map(|_| ModelConfig::parse_latency(value))
                .collect()
        }
    }

    fn parse_latency(value: &str) -> u64 {
        match parse_duration(value) {
            Ok(d) => d.as_millis() as u64,
            Err(_) => value.parse().expect("Illegal numeric value"),
        }
    }

    fn get_python_path(&self) -> Option<&str> {
        let python_path = match self.python_path.as_ref() {
            None => Some("/usr/bin/python3"),
            Some(s) => Some(s.as_str()),
        };
        python_path
    }
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

    let data_points_count = (end - start) as f64;
    let avg = total / data_points_count;
    let mut deviation = 0.;
    for i in start..end {
        let value = *rps_buckets.get(&i).unwrap_or(&0);
        deviation += (avg - value as f64) * (avg - value as f64);
    }

    println!(
        "Avg rate: {:.3}, StdDev: {:.3}",
        avg,
        (deviation / data_points_count).sqrt()
    );

    let line_plot = line_plot::<u64, u64>(x, y, None);
    let mut figure = Figure::new();
    figure.add_plot(line_plot.clone());
    figure.add_plot(line_plot);
    figure.save(
        format!("./request_rate_{}.png", config.name).as_str(),
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
        format!("./latency_histogram_{}.png", config.name).as_str(),
        config.get_python_path(),
    );

    let line_plot = line_plot::<f64, f64>(percentiles_x, percentiles_y, None);
    let mut figure = Figure::new();
    figure.add_plot(line_plot.clone());
    figure.add_plot(line_plot);
    figure.save(
        format!("./latency_percentiles_{}.png", config.name).as_str(),
        config.get_python_path(),
    );
}

fn build_latency_timeline(config: &ModelConfig, mut latencies: Vec<TaskStats>) {
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
        format!("./latency_timeline_{}.png", config.name).as_str(),
        config.get_python_path(),
    );
}
