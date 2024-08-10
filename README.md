# Rust Job Scheduler

A simple job scheduler implemented in Rust, capable of handling job dependencies, timeouts, and job cancellation. This implementation uses Crossbeam channels for communication, a priority queue for job scheduling, and a thread pool for concurrent job execution.

## Features

- **Job Scheduling**: Schedules jobs based on priority and handles job execution.
- **Timeouts**: Supports job timeouts to automatically cancel jobs that exceed a specified duration.
- **Job Dependencies**: Manages job dependencies, ensuring that dependent jobs are executed only after their prerequisites are completed.
- **Cancellation**: Allows jobs to be canceled and handles dependencies accordingly.
- **Thread Pool**: Utilizes a thread pool to execute jobs concurrently, improving performance.

## Dependencies

- **`crossbeam`**: Provides concurrent data structures and communication mechanisms.
- **`log`**: Enables logging of job status and scheduler activities.
- **`env_logger`**: Provides initialization and configuration for logging.

## Usage

1. **Add Dependencies**:
   Add the following to your `Cargo.toml`:

   ```toml
   [dependencies]
   crossbeam = "0.8.4"
   env_logger = "0.11.5"
   log = "0.4.22"
   rand = "0.8.5"
   ```

2. **To run the code**:

```console
$ RUST_LOG=info cargo run
```
