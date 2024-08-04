use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

type JobId = usize;
type JobFn = Box<dyn FnOnce() + Send + 'static>;

struct Job {
    _id: JobId,
    deps: HashSet<JobId>,
    func: JobFn,
}

#[derive(Debug, PartialEq)]
enum JobStatus {
    Pending,
    Running,
    Completed,
}

pub struct JobScheduler {
    jobs: HashMap<JobId, Job>,
    job_statuses: Arc<Mutex<HashMap<JobId, JobStatus>>>,
    ready_jobs: VecDeque<JobId>,
    next_id: JobId,
    threadpool: ThreadPool,
}

impl JobScheduler {
    pub fn new(count: usize) -> Self {
        JobScheduler {
            jobs: HashMap::new(),
            job_statuses: Arc::new(Mutex::new(HashMap::new())),
            ready_jobs: VecDeque::new(),
            next_id: 0,
            threadpool: ThreadPool::new(count),
        }
    }

    pub fn add_job<F>(&mut self, deps: Vec<JobId>, func: F) -> JobId
    where
        F: FnOnce() + Send + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;
        let job = Job {
            _id: id,
            deps: deps.into_iter().collect(),
            func: Box::new(func),
        };

        self.job_statuses
            .lock()
            .unwrap()
            .insert(id, JobStatus::Pending);

        if job.deps.is_empty() {
            self.ready_jobs.push_back(id);
        }
        self.jobs.insert(id, job);

        println!(
            "Added job {}: {:?}",
            id,
            self.job_statuses.lock().unwrap().get(&id)
        );

        id
    }

    pub fn run(&mut self) {
        while !self.ready_jobs.is_empty() || !self.jobs.is_empty() {
            let jobs_to_run: Vec<JobId> = self.ready_jobs.drain(..).collect();

            for job_id in jobs_to_run {
                if let Some(job) = self.jobs.remove(&job_id) {
                    let job_statuses = Arc::clone(&self.job_statuses);
                    self.threadpool.execute(move || {
                        {
                            let mut statuses = job_statuses.lock().unwrap();
                            if let Some(status) = statuses.get_mut(&job_id) {
                                *status = JobStatus::Running;
                            }
                            println!("Job {} is now running", job_id);
                        }

                        (job.func)();

                        {
                            let mut statuses = job_statuses.lock().unwrap();
                            if let Some(status) = statuses.get_mut(&job_id) {
                                *status = JobStatus::Completed;
                            }
                            println!("Job {} has completed", job_id);
                        }
                    });
                } else {
                    println!("Warning: Job {} not found", job_id);
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

        for (&job_id, job) in &self.jobs {
            if job
                .deps
                .iter()
                .all(|&dep_id| matches!(job_statuses.get(&dep_id), Some(JobStatus::Completed)))
            {
                new_ready_jobs.push(job_id);
            }
        }

        drop(job_statuses);

        for job_id in new_ready_jobs {
            if !self.ready_jobs.contains(&job_id) {
                self.ready_jobs.push_back(job_id);
                println!("Job {} is now ready to run", job_id);
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
