use crossbeam::channel::{self, Sender};
use crossbeam::queue::SegQueue;
use crossbeam::sync::ShardedLock;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

type JobId = usize;
type JobFn = Box<dyn FnOnce() -> Result<(), String> + Send + 'static>;
type CancelFn = Box<dyn Fn(JobId) + Send + 'static>;

#[derive(Debug, Clone)]
struct Job {
    id: JobId,
    deps: HashSet<JobId>,
    priority: usize,
    timeout: Option<Duration>,
    submitted_at: Instant,
}

impl Ord for Job {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| self.submitted_at.cmp(&other.submitted_at))
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

#[derive(Debug, PartialEq)]
enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Canceled,
}

pub struct JobScheduler {
    jobs: HashMap<JobId, Job>,
    job_statuses: Arc<ShardedLock<HashMap<JobId, JobStatus>>>,
    ready_jobs: Arc<SegQueue<Job>>,
    ready_job_ids: HashSet<JobId>,
    next_id: JobId,
    threadpool: ThreadPool,
    job_funcs: HashMap<JobId, JobFn>,
    cancel_callback: Option<CancelFn>,
}

#[allow(dead_code)]
impl JobScheduler {
    pub fn new(count: usize) -> Self {
        JobScheduler {
            jobs: HashMap::new(),
            job_statuses: Arc::new(ShardedLock::new(HashMap::new())),
            ready_jobs: Arc::new(SegQueue::new()),
            ready_job_ids: HashSet::new(),
            next_id: 1,
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

    pub fn add_job<F>(
        &mut self,
        deps: Vec<JobId>,
        func: F,
        priority: usize,
        timeout: Duration,
    ) -> JobId
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;
        let job = Job {
            id,
            deps: deps.clone().into_iter().collect(),
            priority,
            timeout: Some(timeout),
            submitted_at: Instant::now(),
        };

        self.job_statuses
            .write()
            .unwrap()
            .insert(id, JobStatus::Pending);

        println!(
            "Added job {}: Priority {}, Dependencies: {:?}",
            id, job.priority, deps
        );

        if job.deps.is_empty() && !self.ready_job_ids.contains(&id) {
            self.ready_jobs.push(job.clone());
            self.ready_job_ids.insert(id);
            println!("Job {} is ready to run", id);
        }

        self.jobs.insert(id, job.clone());
        self.job_funcs.insert(id, Box::new(func));

        id
    }

    pub fn run(&mut self) {
        while !self.ready_jobs.is_empty() || !self.jobs.is_empty() {
            let jobs_to_run: Vec<Job> = self.ready_jobs.pop().into_iter().collect();

            for job in jobs_to_run {
                if let Some(job_func) = self.job_funcs.remove(&job.id) {
                    let job_statuses = Arc::clone(&self.job_statuses);
                    let timeout = job.timeout;

                    self.threadpool.execute(move || {
                        let start_time = Instant::now();
                        {
                            let mut statuses = job_statuses.write().unwrap();
                            statuses.insert(job.id, JobStatus::Running);
                            println!("Job {} status changed to Running", job.id);
                        }
                        let result = job_func();

                        let elapsed = start_time.elapsed();
                        {
                            let mut statuses = job_statuses.write().unwrap();
                            match result {
                                Err(_) => {
                                    statuses.insert(job.id, JobStatus::Failed);
                                    Err(format!("Job {} status changed to Failed", job.id))
                                }
                                Ok(_) if timeout.map_or(false, |t| elapsed > t) => {
                                    statuses.insert(job.id, JobStatus::Canceled);
                                    println!("Job {} status changed to Canceled", job.id);
                                    Ok(())
                                }
                                Ok(_) => {
                                    statuses.insert(job.id, JobStatus::Completed);
                                    println!("Job {} status changed to Completed", job.id);
                                    Ok(())
                                }
                            }
                        }
                    });
                }
            }

            self.update_ready_jobs();
            thread::yield_now();
        }

        while self
            .job_statuses
            .read()
            .unwrap()
            .values()
            .any(|status| *status != JobStatus::Completed)
        {
            thread::yield_now();
        }

        println!("All jobs have been completed");
    }

    fn update_ready_jobs(&mut self) {
        let job_statuses = self.job_statuses.read().unwrap();
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
                    "Job {} (priority {}) added to ready queue",
                    job.id, job.priority
                );
            }
        }
    }

    pub fn cancel_job(&mut self, job_id: JobId) {
        if let Some(_job) = self.jobs.remove(&job_id) {
            self.job_statuses
                .write()
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

            self.handle_failed_dependencies(job_id);
        } else {
            println!("Job {} not found", job_id);
        }
    }

    fn handle_failed_dependencies(&mut self, failed_job_id: JobId) {
        let mut dependent_jobs = Vec::new();

        for (id, job) in &self.jobs {
            if job.deps.contains(&failed_job_id) {
                dependent_jobs.push(id.clone());
            }
        }

        for dep_job_id in dependent_jobs {
            if let Some(_) = self.jobs.get(&dep_job_id) {
                println!(
                    "Handling failure of Job {} due to dependency failure of Job {}",
                    dep_job_id, failed_job_id
                );
                self.cancel_job(dep_job_id);
            }
        }
    }
}

enum Message {
    NewJob(JobFn),
    Terminate,
}

struct ThreadPool {
    workers: Vec<thread::JoinHandle<()>>,
    sender: Sender<Message>,
}

impl ThreadPool {
    fn new(size: usize) -> Self {
        let (sender, receiver) = channel::unbounded();
        let receiver = Arc::new(ShardedLock::new(receiver));
        let mut workers = Vec::with_capacity(size);

        for i in 0..size {
            let receiver = Arc::clone(&receiver);
            let worker = thread::spawn(move || loop {
                let msg = receiver.read().unwrap().recv().unwrap();
                match msg {
                    Message::NewJob(job) => {
                        println!("Worker {} received a new job", i);
                        if let Err(e) = job() {
                            println!("Worker {} encountered an error: {}", i, e);
                        }
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

    fn execute<F>(&self, job: F)
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        self.sender.send(Message::NewJob(Box::new(job))).unwrap();
    }

    fn shutdown(&mut self) {
        for _ in &self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        for worker in self.workers.drain(..) {
            worker.join().unwrap();
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}
