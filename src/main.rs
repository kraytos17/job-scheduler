use scheduler::JobScheduler;
use std::thread;
use std::time::Duration;
mod scheduler;

fn main() {
    let mut scheduler = JobScheduler::new(4);
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

    let task3 = scheduler.add_job(
        vec![task1, task2],
        || {
            println!("Task 3 executing (depends on 1 and 2)");
            thread::sleep(Duration::from_secs(1));
        },
        3,
    );

    let _task4 = scheduler.add_job(
        vec![task3],
        || {
            println!("Task 4 executing (depends on 3)");
            thread::sleep(Duration::from_secs(1));
        },
        4,
    );

    scheduler.run();
    println!("All tasks completed");
}
