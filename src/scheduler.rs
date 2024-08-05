use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{panic, thread};

type JobId = usize;
type JobFn = Box<dyn FnOnce() + Send + 'static>;
type CancelFn = Box<dyn Fn(JobId) + Send + 'static>;

#[derive(Debug, Clone)]
struct Job {
    id: JobId,
    deps: HashSet<JobId>,
    priority: usize,
    timeout: Option<Duration>,
}

impl Ord for Job {
    fn cmp(&self, other: &Self) -> Ordering {
        other.priority.cmp(&self.priority)
    }
}

impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Job {}

pub struct JobScheduler {
    jobs: HashMap<JobId, Job>,
    job_statuses: Arc<Mutex<HashMap<JobId, JobStatus>>>,
    job_retries: HashMap<JobId, usize>,
    max_retries: usize,
    ready_jobs: BinaryHeap<Job>,
    ready_job_ids: HashSet<JobId>,
    next_id: JobId,
    threadpool: ThreadPool,
    job_funcs: HashMap<JobId, JobFn>,
    cancel_callback: Option<CancelFn>,
}

impl JobScheduler {
    pub fn new(count: usize, max_retries: usize) -> Self {
        JobScheduler {
            jobs: HashMap::new(),
            job_statuses: Arc::new(Mutex::new(HashMap::new())),
            job_retries: HashMap::new(),
            max_retries,
            ready_jobs: BinaryHeap::new(),
            ready_job_ids: HashSet::new(),
            next_id: 0,
            threadpool: ThreadPool::new(count),
            job_funcs: HashMap::new(),
            cancel_callback: None,
        }
    }

    pub fn set_cancel_callback<F>(&mut self, callback: F)
    where
        F: Fn(JobId) + Send + 'static,
    {
        self.cancel_callback = Some(Box::new(callback));
    }

    pub fn add_job<F>(&mut self, deps: Vec<JobId>, func: F, priority: usize, timeout: Duration) -> JobId
    where
        F: FnOnce() + Send + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;
        let job = Job {
            id,
            deps: deps.into_iter().collect(),
            priority,
            timeout: Some(timeout),
        };

        self.job_statuses
            .lock()
            .unwrap()
            .insert(id, JobStatus::Pending);

        if job.deps.is_empty() && !self.ready_job_ids.contains(&id) {
            self.ready_jobs.push(job.clone());
            self.ready_job_ids.insert(id);
            println!("Job {} (priority {}) is now ready to run", id, job.priority);
        }
        self.jobs.insert(id, job.clone());
        self.job_funcs.insert(id, Box::new(func));

        println!(
            "Added job {}: Priority {}, Status: {:?}",
            id,
            job.priority,
            self.job_statuses.lock().unwrap().get(&id)
        );

        id
    }

    pub fn run(&mut self) {
        while !self.ready_jobs.is_empty() || !self.jobs.is_empty() {
            let jobs_to_run: Vec<Job> = self.ready_jobs.drain().collect();

            for job in jobs_to_run {
                if let Some(job_func) = self.job_funcs.remove(&job.id) {
                    let job_statuses = Arc::clone(&self.job_statuses);
                    let timeout = job.timeout;
                    println!(
                        "Dispatching Job {} (priority {}) to threadpool",
                        job.id, job.priority
                    );
                    self.threadpool.execute(move || {
                        let start_time = Instant::now();
                        let result = panic::catch_unwind(AssertUnwindSafe(|| {
                            job_func();
                        }));

                        let elapsed = start_time.elapsed();
                        {
                            let mut statuses = job_statuses.lock().unwrap();
                            if result.is_err() {
                                statuses.insert(job.id, JobStatus::Failed);
                            } else if timeout.map_or(false, |t| elapsed > t) {
                                statuses.insert(job.id, JobStatus::Canceled);
                            } else {
                                statuses.insert(job.id, JobStatus::Completed);
                            }
                        }
                        println!(
                            "Job {} has completed with status: {:?}",
                            job.id,
                            if result.is_err() {
                                "Failed"
                            } else if timeout.map_or(false, |t| elapsed > t) {
                                "Canceled"
                            } else {
                                "Completed"
                            }
                        );
                    });
                }
            }

            self.update_ready_jobs();
            thread::yield_now();
        }

        while self
            .job_statuses
            .lock()
            .unwrap()
            .values()
            .any(|status| *status != JobStatus::Completed)
        {
            thread::yield_now();
        }

        println!("All jobs have been completed");
    }

    fn update_ready_jobs(&mut self) {
        let job_statuses = self.job_statuses.lock().unwrap();
        let mut new_ready_jobs = Vec::new();

        for (_, job) in &self.jobs {
            if job
                .deps
                .iter()
                .all(|&dep_id| matches!(job_statuses.get(&dep_id), Some(JobStatus::Completed)))
            {
                new_ready_jobs.push(job.clone());
            }
        }

        drop(job_statuses);

        for job in new_ready_jobs {
            if !self.ready_job_ids.contains(&job.id) {
                self.ready_jobs.push(job.clone());
                self.ready_job_ids.insert(job.id);
                println!(
                    "Job {} (priority {}) is now ready to run",
                    job.id, job.priority
                );
            }
        }
    }

    pub fn cancel_job(&mut self, job_id: JobId) {
        if let Some(_job) = self.jobs.remove(&job_id) {
            self.ready_jobs.retain(|j| j.id != job_id);
            self.ready_job_ids.remove(&job_id);
            self.job_statuses
                .lock()
                .unwrap()
                .insert(job_id, JobStatus::Canceled);

            for (_, job) in &mut self.jobs {
                if job.deps.contains(&job_id) {
                    job.deps.remove(&job_id);
                    if job.deps.is_empty() {
                        self.ready_jobs.push(job.clone());
                        self.ready_job_ids.insert(job.id);
                    }
                }
            }

            println!("Job {} has been canceled", job_id);

            if let Some(ref callback) = self.cancel_callback {
                callback(job_id);
            }
        } else {
            println!("Job {} not found", job_id);
        }
    }

    pub fn handle_job_failure(&mut self, job_id: JobId) {
        let retry_count = self.job_retries.entry(job_id).or_insert(0);
        if *retry_count < self.max_retries {
            *retry_count += 1;
            if let Some(job) = self.jobs.get(&job_id) {
                self.ready_jobs.push(job.clone());
                self.ready_job_ids.insert(job_id);
                self.job_statuses
                    .lock()
                    .unwrap()
                    .insert(job_id, JobStatus::Pending);
                println!("Retrying Job {} (attempt {})", job_id, *retry_count + 1);
            } else {
                self.job_statuses
                    .lock()
                    .unwrap()
                    .insert(job_id, JobStatus::Failed);
                println!(
                    "Job {} has failed after {} retries",
                    job_id, self.max_retries
                );
            }
        }
    }
}

struct ThreadPool {
    workers: Vec<thread::JoinHandle<()>>,
    sender: Sender<Message>,
}

enum Message {
    NewJob(JobFn),
    Terminate,
}

#[derive(Debug, PartialEq)]
enum JobStatus {
    Pending,
    Completed,
    Failed,
    Canceled,
}

impl ThreadPool {
    fn new(size: usize) -> Self {
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(size);

        for i in 0..size {
            let receiver = Arc::clone(&receiver);
            let worker = thread::spawn(move || loop {
                let msg = receiver.lock().unwrap().recv().unwrap();
                match msg {
                    Message::NewJob(job) => {
                        println!("Worker {} received a new job", i);
                        job()
                    }
                    Message::Terminate => {
                        println!("Worker {} is terminating", i);
                        break;
                    }
                }
            });
            workers.push(worker);
        }

        ThreadPool { workers, sender }
    }

    fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.send(Message::NewJob(job)).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        for _ in &self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }
        for worker in self.workers.drain(..) {
            worker.join().unwrap();
        }
    }
}
