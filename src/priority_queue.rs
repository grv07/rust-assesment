use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::time::{sleep_until, Instant as TokioInstant};

pub type RetryQueue<T> = BTreeMap<Instant, Vec<T>>;

pub async fn retry_scheduler_loop<T>(queue: Arc<Mutex<RetryQueue<T>>>) {
    loop {
        let next_due = {
            let q = queue.lock().unwrap();
            q.keys().next().cloned()
        };

        if let Some(due_time) = next_due {
            let now = Instant::now();

            if due_time > now {
                sleep_until(TokioInstant::from_std(due_time)).await;
            }

            let mut tasks = vec![];
            {
                let mut q = queue.lock().unwrap();
                if let Some(mut due_reqs) = q.remove(&due_time) {
                    tasks.append(&mut due_reqs);
                }
            }

            for req in tasks {
                // tokio::spawn(process_request_with_retry_logic(req, queue.clone()));
                todo!()
            }
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
