mod scheduler;

use scheduler::JobScheduler;
use std::thread;
use std::time::Duration;

fn main() {
    let mut scheduler = JobScheduler::new(4, 3);

    let task1 = scheduler.add_job(
        vec![],
        || {
            println!("Task 1 executing");
            thread::sleep(Duration::from_secs(1));
        },
        1,
    );

    let task2 = scheduler.add_job(
        vec![],
        || {
            println!("Task 2 executing");
            thread::sleep(Duration::from_secs(1));
        },
        2,
    );

    let _task3 = scheduler.add_job(
        vec![task1],
        || {
            println!("Task 3 executing (depends on 1 and 2)");
            thread::sleep(Duration::from_secs(1));
        },
        3,
    );

    let _task4 = scheduler.add_job(
        vec![task2],
        || {
            println!("Task 4 executing (depends on 3)");
            thread::sleep(Duration::from_secs(1));
        },
        4,
    );

    scheduler.cancel_job(task2);
    scheduler.run();
    println!("All tasks completed");
}
