### Benchmarks

These benchmarks compare the difference between manipulating data in the following modes:

* Multi-thread batched atomic
* Multi-thread naïve atomic
* Multi-thread naïve mutex

The benchmarks are artificial, but the goal is to give a general idea about the cost of consistency guarantees.

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
but designed to hide from the compiler the fact that it's all `1`s to add:

```rust
fn sum_batched(observations: &[usize], counter: &AtomicUsize) {
    let mut batch = 0;
    for i in observations {
        batch += i | 1;
    }
    counter.fetch_add(batch, Ordering::Relaxed);
}
```

The difference is not as prominent, but of the same order:

```
Sum Batched             time:   0.1313 us                        
Sum Naive Atomic        time:   5.5989 us                              
Sum Naive Mutex         time:   21.709 us                             

Batched      71474881 operations, 1.000000
Atomic        1937375 operations, 0.027106
Mutex          494443 operations, 0.006918
```

`Batched` is faster than `Atomic` ~37 times, and faster than `Mutex` ~145 times.  

