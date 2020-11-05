[![Crate](https://img.shields.io/crates/v/concurrency-demo-benchmarks.svg)](https://crates.io/crates/concurrency-demo-benchmarks)
![Clippy/Fmt](https://github.com/xnuter/concurrency-demo-benchmarks/workflows/Clippy/Fmt/badge.svg)

### Overview

A small utility to benchmark different approaches for building concurrent applications.

#### Pre-requisites

1. `cargo` - https://www.rust-lang.org/tools/install
1. `python3.6+` with `matplotlib`

It generates three files in the `./figures` directory:

* `latency_histogram_{name}.png`
* `latency_percentiles_{name}.png`
* `request_rate_{name}.png`

where `{name}` is the `--name` (or `-N`) parameter value.

You may need to use `--pythob`/`-p` parameter to specify `python3` binary, if it's not in `/usr/local/bin/python3`. E.g.

```
concurrency-demo-benchmarks --name async_30s \
                            --rate 1000 \
                            --num_req 100000 \
                            --latency "200*9,30000" \
                            --python /usr/bin/python3 \
                            async
```

#### Installation

```
cargo install concurrency-demo-benchmarks  
```


#### Run batched/atomic/mutex increments benchmark
```
git clone https://github.com/xnuter/concurrency-demo-benchmarks.git
cargo bench
```

#### Command line options

```
A tool to model sync vs async processing for a network service

USAGE:
    concurrency-demo-benchmarks [OPTIONS] --name <NAME> --rate <RATE> --num_req <NUM_REQUESTS> --latency <LATENCY_DISTRIBUTION> [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -l, --latency <LATENCY_DISTRIBUTION>    Comma separated latency values. E.g. 200,200,200,500
    -N, --name <NAME>                       Name of the test-case
    -n, --num_req <NUM_REQUESTS>            Number of requests. E.g. 1000
    -p, --python_path <PYTHON_PATH>         Optional path to python3, e.g. /usr/bin/python3
    -r, --rate <RATE>                       Request rate per second. E.g. 100 or 1000

SUBCOMMANDS:
    async    Model a service with Async I/O
    help     Prints this message or the help of the given subcommand(s)
    sync     Model a service with Blocking I/O

```
#### Run sync demo
* 1000 rps
* 200ms latency, 10 endpoints
* 500 threads
```
concurrency-demo-benchmarks --name sync_t500_200ms \
                            --rate 1000 \
                            --num_req 10000 \
                            --latency "200*10" \
                            sync --threads 500
```

* 1000 rps
* 600ms latency (stable)
* 500 threads
```
concurrency-demo-benchmarks --name sync_t500_600ms \
                            --rate 1000 \
                            --num_req 10000 \
                            --latency "600*10" \
                            sync --threads 500
```

* 1000 rps
* 200ms latency but 30s for 10%
* 500 threads
```
concurrency-demo-benchmarks --name sync_t500_30s \
                            --rate 1000 \
                            --num_req 100000 \
                            --latency "200*9,30000" \
                            sync --threads 500
```

#### Run async demo
* 1000 rps
* 200ms latency (stable)
```
concurrency-demo-benchmarks --name async_200ms \
                            --rate 1000 \
                            --num_req 10000 \
                            --latency "200*10" \
                            async
```

* 1000 rps
* 600ms latency (stable)
```
concurrency-demo-benchmarks --name async_600ms \
                            --rate 1000 \
                            --num_req 100000 \
                            --latency "600*10" \
                            async
```

* 1000 rps
* 200ms latency but 30s for 10%
```
concurrency-demo-benchmarks --name async_30s \
                            --rate 1000 \
                            --num_req 100000 \
                            --latency "200*9,30000" \
                            async
```
