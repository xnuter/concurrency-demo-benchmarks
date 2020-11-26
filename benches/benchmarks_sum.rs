use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn sum_batched(observations: &[usize], counter: &AtomicUsize) {
    let mut batch = 0;
    for i in observations {
        batch += i | 1;
    }
    counter.fetch_add(batch, Ordering::Relaxed);
}

fn sum_naive_atomic(observations: &[usize], counter: &AtomicUsize) {
    for i in observations {
        counter.fetch_add(*i | 1, Ordering::Relaxed);
    }
}

fn sum_naive_mutex(observations: &[usize], counter_mutex: &Mutex<usize>) {
    for i in observations {
        let mut lock = counter_mutex.lock().expect("Never fails in this bench");
        *lock += *i | 1;
    }
}

fn benchmark_increment(c: &mut Criterion) {
    let counter_batched: AtomicUsize = AtomicUsize::new(0);
    let counter_atomic: AtomicUsize = AtomicUsize::new(0);
    let counter_mutex: Mutex<usize> = Mutex::new(0);

    let repetitions = 1_000;
    let vec = (0..repetitions).map(|i| i % 2).collect::<Vec<usize>>();
    let increment = vec.as_slice();

    c.bench_function("Sum Batched", |b| {
        b.iter(|| black_box(sum_batched(increment, &counter_batched)))
    });
    c.bench_function("Sum Naive Atomic", |b| {
        b.iter(|| black_box(sum_naive_atomic(increment, &counter_atomic)))
    });
    c.bench_function("Sum Naive Mutex", |b| {
        b.iter(|| black_box(sum_naive_mutex(increment, &counter_mutex)))
    });

    let batched = counter_batched.load(Ordering::Relaxed);
    let atomic = counter_atomic.load(Ordering::Relaxed);
    let mutex = counter_mutex.lock().unwrap();
    println!(
        "Sum Batched  {:12} operations, {:.6}",
        batched / repetitions,
        batched as f64 / batched as f64
    );
    println!(
        "Sum Atomic   {:12} operations, {:.6}",
        atomic / repetitions,
        atomic as f64 / batched as f64
    );
    println!(
        "Sum Mutex    {:12} operations, {:.6}",
        *mutex / repetitions,
        *mutex as f64 / batched as f64
    );
}

criterion_group!(benches, benchmark_increment,);

criterion_main!(benches);
