mod scheduler;

use scheduler::JobScheduler;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let scheduler = Arc::new(Mutex::new(JobScheduler::new(4)));
    let scheduler_clone = Arc::clone(&scheduler);
    scheduler_clone
        .lock()
        .unwrap()
        .set_cancel_callback(|job_id| {
            println!("Callback: Job {} was canceled", job_id);
        });

    let job1_id = {
        let mut scheduler = scheduler.lock().unwrap();
        scheduler.add_job(
            vec![],
            || {
                println!("Job 1 running");
                thread::sleep(Duration::from_secs(2));
                Ok(())
            },
            1,
            Duration::from_secs(5),
        )
    };

    let job2_id = {
        let mut scheduler = scheduler.lock().unwrap();
        scheduler.add_job(
            vec![],
            || {
                println!("Job 2 running");
                thread::sleep(Duration::from_secs(3));
                Ok(())
            },
            2,
            Duration::from_secs(10),
        )
    };

    let job3_id = {
        let mut scheduler = scheduler.lock().unwrap();
        scheduler.add_job(
            vec![job1_id, job2_id],
            || {
                println!("Job 3 running (depends on Job 1 and Job 2)");
                thread::sleep(Duration::from_secs(1));
                Ok(())
                //Err("Job 3 failed".to_string())
            },
            3,
            Duration::from_secs(5),
        )
    };

    let _job4_id = {
        let mut scheduler = scheduler.lock().unwrap();
        scheduler.add_job(
            vec![job3_id],
            || {
                println!("Job 4 running (depends on Job 3)");
                thread::sleep(Duration::from_secs(1));
                Ok(())
            },
            1,
            Duration::from_secs(5),
        )
    };

    let scheduler_clone = Arc::clone(&scheduler);
    let scheduler_handle = thread::spawn(move || {
        let mut scheduler = scheduler_clone.lock().unwrap();
        scheduler.run();
    });

    // thread::sleep(Duration::from_secs(1));
    // println!("Handling job failure for Job 3...");
    // {
    //     let mut scheduler = scheduler.lock().unwrap();
    //     scheduler.handle_job_failure(job3_id);
    // }

    // thread::sleep(Duration::from_secs(1));
    // println!("Canceling Job 4...");
    // {
    //     let mut scheduler = scheduler.lock().unwrap();
    //     scheduler.cancel_job(job4_id);
    // }

    scheduler_handle.join().unwrap();
}
