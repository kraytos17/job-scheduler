mod scheduler;

use scheduler::JobScheduler;
use std::thread;
use std::time::Duration;

fn main() {
    let mut scheduler = JobScheduler::new(4, 3);
    scheduler.set_cancel_callback(|job_id| {
        println!("Notification: Job {} has been canceled", job_id);
    });

    let job0 = scheduler.add_job(
        vec![],
        || {
            println!("Job 0 executing");
            thread::sleep(Duration::from_secs(1));
        },
        1,
    );

    let job1 = scheduler.add_job(
        vec![],
        || {
            println!("Job 1 executing");
            thread::sleep(Duration::from_secs(1));
        },
        2,
    );

    let job2 = scheduler.add_job(
        vec![job0, job1],
        || {
            println!("Job 2 executing (depends on 0 and 1)");
            thread::sleep(Duration::from_secs(1));
        },
        3,
    );

    let _job3 = scheduler.add_job(
        vec![job2],
        || {
            println!("Job 3 executing (depends on 2)");
            thread::sleep(Duration::from_secs(1));
        },
        4,
    );

    scheduler.cancel_job(job1);
    scheduler.run();
    println!("All tasks completed");
}
