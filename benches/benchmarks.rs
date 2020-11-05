use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn benchmark_batched_increment(repetitions: usize, mut increment: usize, counter: &AtomicUsize) {
    let mut batch = 0;
    for _ in 0..repetitions {
        // avoiding compiler optimizations
        // E.g. go to https://rust.godbolt.org/z/7he65h
        // and try to comment the line #4
        increment = increment ^ 1;
        batch += increment;
    }
    counter.fetch_add(batch, Ordering::Relaxed);
}

fn benchmark_atomic_increment(repetitions: usize, mut increment: usize, counter: &AtomicUsize) {
    for _ in 0..repetitions {
        increment = increment ^ 1;
        counter.fetch_add(increment, Ordering::Relaxed);
    }
}

fn benchmark_mutex_increment(
    repetitions: usize,
    mut increment: usize,
    counter_mutex: &Mutex<usize>,
) {
    for _ in 0..repetitions {
        increment = increment ^ 1;
        let mut lock = counter_mutex.lock().expect("Never fails in this bench");
        *lock += increment;
    }
}

fn benchmark_increment(c: &mut Criterion) {
    let counter_batched: AtomicUsize = AtomicUsize::new(0);
    let counter_atomic: AtomicUsize = AtomicUsize::new(0);
    let counter_mutex: Mutex<usize> = Mutex::new(0);

    let increment = 1;
    let repetitions = 1000;

    c.bench_function("Increment Batched", |b| {
        b.iter(|| {
            black_box(benchmark_batched_increment(
                repetitions,
                increment,
                &counter_batched,
            ))
        })
    });
    c.bench_function("Increment Atomic", |b| {
        b.iter(|| {
            black_box(benchmark_atomic_increment(
                repetitions,
                increment,
                &counter_atomic,
            ))
        })
    });
    c.bench_function("Increment Mutex", |b| {
        b.iter(|| {
            black_box(benchmark_mutex_increment(
                repetitions,
                increment,
                &counter_mutex,
            ))
        })
    });

    let batched = counter_batched.load(Ordering::Relaxed);
    let atomic = counter_atomic.load(Ordering::Relaxed);
    let mutex = counter_mutex.lock().unwrap();
    println!(
        "Batched  {:12} operations, {:.6}",
        batched / repetitions,
        batched as f64 / batched as f64
    );
    println!(
        "Atomic   {:12} operations, {:.6}",
        atomic / repetitions,
        atomic as f64 / batched as f64
    );
    println!(
        "Mutex    {:12} operations, {:.6}",
        *mutex / repetitions,
        *mutex as f64 / batched as f64
    );
}

criterion_group!(benches, benchmark_increment,);

criterion_main!(benches);
