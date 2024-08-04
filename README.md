# Custom Job Scheduler

This is a custom job scheduler implemented in Rust that supports job prioritization, dependency management, and concurrent execution using a thread pool.

## Features

- Job Prioritization: Jobs are prioritized based on a priority value. Higher priority jobs are scheduled before lower priority ones.

- Job Dependencies: Jobs can have dependencies on other jobs. A job is only executed once all its dependencies are completed.

- Thread Pool: Executes jobs concurrently using a pool of worker threads.

- Logging: Provides detailed logging for job scheduling, execution, and completion.
