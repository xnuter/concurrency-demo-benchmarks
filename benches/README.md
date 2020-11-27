### Benchmarks

These benchmarks compare the difference between manipulating data in the following modes:

* Multi-thread batched atomic
* Multi-thread naïve atomic
* Multi-thread naïve mutex

The benchmarks are artificial, but the goal is to give a general idea about the cost of consistency guarantees.

All benchmarks are performed on `Intel(R) Xeon(R) CPU @ 2.30GHz`.

### Benchmark 1. Incrementing data

```rust
fn benchmark_batched_increment(repetitions: usize, mut increment: usize, counter: &AtomicUsize) {
    let mut batch = 0;
    for _ in 0..repetitions {
        // avoiding compiler optimizations
        // E.g. go to https://rust.godbolt.org/z/7he65h
        // and try to comment the line #4
        increment ^= 1;
        batch += increment;
    }
    counter.fetch_add(batch, Ordering::Relaxed);
}
```

The `increment ^= 1;` statement was introduced to avoid optimizations.
However, it turned out that some optimizations still have place:
https://stackoverflow.com/questions/65010708/why-is-xor-much-faster-than-or

That's why another scenario was benchmarked to sum an array.

### Benchmark 2. Summing an array

Semantically, this is the same operation - as for the benchmark 1,
but designed to hide from the compiler the fact that it's all `1`s to add and make it more _realistic_:

```rust
fn sum_batched(observations: &[usize], counter: &AtomicUsize) {
    let mut batch = 0;
    for i in observations {
        batch += i;
    }
    counter.fetch_add(batch, Ordering::Relaxed);
}

fn sum_naive_atomic(observations: &[usize], counter: &AtomicUsize) {
    for i in observations {
        counter.fetch_add(*i, Ordering::Relaxed);
    }
}

fn sum_naive_mutex(observations: &[usize], counter_mutex: &Mutex<usize>) {
    for i in observations {
        let mut lock = counter_mutex.lock().expect("Never fails in this bench");
        *lock += *i;
    }
}
```

The difference is not as prominent, but of the same order:

```
Sum Batched             time:   0.1149 us
Sum Naive Atomic        time:   6.7829 us
Sum Naive Mutex         time:   21.455 us 

Sum Batched      76848081 operations, 1.000000
Sum Atomic        1261587 operations, 0.016417
Sum Mutex          494443 operations, 0.006434
```

`Batched` is faster than `Atomic` ~61 times, and faster than `Mutex` ~155 times.  

