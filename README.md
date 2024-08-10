# Rust Job Scheduler

A simple job scheduler implemented in Rust, capable of handling job dependencies, timeouts, and job cancellation. This implementation uses crossbeam channels for communication, a priority queue for job scheduling, and a thread pool for concurrent job execution.

## Features

- **Job Scheduling**: Handles job scheduling based on priority and dependencies.
- **Timeouts**: Supports job timeouts, allowing jobs to be canceled if they exceed a specified duration.
- **Job Dependencies**: Manages job dependencies, ensuring that dependent jobs are only executed after their prerequisites are completed.
- **Cancellation**: Provides functionality to cancel jobs and handle dependent jobs appropriately.
- **Thread Pool**: Utilizes a thread pool for concurrent job execution.

## Dependencies

- `crossbeam` for concurrent data structures and communication.
- `std` for standard Rust libraries.
