use crossbeam::channel::{self, Sender};
use crossbeam::queue::SegQueue;
use crossbeam::sync::ShardedLock;
use log::{error, info, warn};
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

#[derive(Debug, PartialEq, Clone)]
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
    last_log_time: Instant,
    log_interval: Duration,
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
            last_log_time: Instant::now(),
            log_interval: Duration::from_secs(10),
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

        info!("{} {} {:?} {}", id, job.priority, deps, "Added job");

        if job.deps.is_empty() && !self.ready_job_ids.contains(&id) {
            self.ready_jobs.push(job.clone());
            self.ready_job_ids.insert(id);
            info!("{} {}", id, "Job is ready to run");
        }

        self.jobs.insert(id, job.clone());
        self.job_funcs.insert(id, Box::new(func));
        self.log_queue_contents();

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
                            info!("{} {}", job.id, "Job status changed to Running");
                        }

                        let result = job_func();
                        let elapsed = start_time.elapsed();
                        let status = match result {
                            Err(_) => JobStatus::Failed,
                            Ok(_) if timeout.map_or(false, |t| elapsed > t) => JobStatus::Canceled,
                            Ok(_) => JobStatus::Completed,
                        };

                        {
                            let mut statuses = job_statuses.write().unwrap();
                            statuses.insert(job.id, status.clone());
                            info!("{} {:?} {}", job.id, status, "Job status changed");
                        }

                        Ok(())
                    });
                }
            }

            self.update_ready_jobs();
            self.log_queue_contents();
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

        info!("All jobs have been completed");
    }

    fn update_ready_jobs(&mut self) {
        let job_statuses = self.job_statuses.read().unwrap();
        let mut new_ready_jobs = Vec::new();

        for job in self.jobs.values() {
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
                info!("{} {} {}", job.id, job.priority, "Job added to ready queue");
            }
        }

        self.log_queue_contents();
    }

    pub fn cancel_job(&mut self, job_id: JobId) {
        if let Some(_job) = self.jobs.remove(&job_id) {
            self.job_statuses
                .write()
                .unwrap()
                .insert(job_id, JobStatus::Canceled);

            for job in self.jobs.values_mut() {
                if job.deps.contains(&job_id) {
                    job.deps.remove(&job_id);
                    if job.deps.is_empty() {
                        self.ready_jobs.push(job.clone());
                        self.ready_job_ids.insert(job.id);
                    }
                }
            }

            info!("{} {}", job_id, "Job has been canceled");

            if let Some(ref callback) = self.cancel_callback {
                callback(job_id);
            }

            self.handle_failed_dependencies(job_id);
            self.log_queue_contents();
        } else {
            warn!("{} {}", job_id, "Job not found");
        }
    }

    fn handle_failed_dependencies(&mut self, failed_job_id: JobId) {
        let mut dependent_jobs = Vec::new();
        for (id, job) in &self.jobs {
            if job.deps.contains(&failed_job_id) {
                dependent_jobs.push(*id);
            }
        }

        for dep_job_id in dependent_jobs {
            if let Some(job) = self.jobs.get_mut(&dep_job_id) {
                warn!(
                    "{} {} {}",
                    dep_job_id, failed_job_id, "Handling failure of job due to dependency failure"
                );

                job.deps.remove(&failed_job_id);
                if job.deps.is_empty() {
                    self.ready_jobs.push(job.clone());
                    self.ready_job_ids.insert(job.id);
                    info!(
                        "{} {} {}",
                        job.id, job.priority, "Job added to ready queue due to failed dependency"
                    );
                }
            }
        }

        self.log_queue_contents();
    }

    fn log_queue_contents(&mut self) {
        if Instant::now().duration_since(self.last_log_time) >= self.log_interval {
            let ready_jobs: Vec<JobId> = self.ready_job_ids.iter().cloned().collect();
            info!("Queue contents: {:?}", ready_jobs);
            let statuses = self.job_statuses.read().unwrap();
            info!("Job statuses: {:?}", *statuses);
            self.last_log_time = Instant::now();
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
                        info!("{} {}", i, "Worker received a new job");
                        if let Err(e) = job() {
                            error!("{} {} {}", i, "Worker encountered an error: {}", e);
                        }
                    }
                    Message::Terminate => {
                        info!("{} {}", i, "Worker is terminating");
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
