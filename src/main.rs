mod scheduler;

use scheduler::JobScheduler;
use std::time::Duration;

fn main() {
    let mut scheduler = JobScheduler::new(4, 3);
    scheduler.set_cancel_callback(|job_id| {
        println!("Callback: Job {} was canceled", job_id);
    });

    let job0 = scheduler.add_job(
        vec![],
        || {
            println!("Executing job 0");
            std::thread::sleep(Duration::from_secs(2));
        },
        1,
        Duration::from_secs(3),
    );

    let job1 = scheduler.add_job(
        vec![job0],
        || {
            println!("Executing job 1");
            if rand::random::<bool>() {
                panic!("Job 1 failed!");
            }
            std::thread::sleep(Duration::from_secs(2));
        },
        2,
        Duration::from_secs(3),
    );

    let _job2 = scheduler.add_job(
        vec![job1],
        || {
            println!("Executing job 2");
            std::thread::sleep(Duration::from_secs(2));
        },
        3,
        Duration::from_secs(3),
    );

    scheduler.run();
    println!("All jobs have been processed");
}
