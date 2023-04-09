use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::{AcquireError, Semaphore};
use tokio::task::JoinError;

pub type Task<T> = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = T> + Send>>>;

pub async fn create_queue<T, U>(tasks: T, max_workers: NonZeroUsize) -> Result<Vec<U>, QueueError>
where
    T: IntoIterator<Item = Task<U>>,
    U: Send + 'static,
{
    let sem = Arc::new(Semaphore::new(max_workers.into()));
    let mut handles = Vec::new();

    for task in tasks {
        let permit = Arc::clone(&sem).acquire_owned().await?;
        let future = task();
        let handle = tokio::task::spawn(async move {
            let result = future.await;
            drop(permit);
            result
        });
        handles.push(handle);
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        let result = handle.await?;
        results.push(result);
    }
    Ok(results)
}

#[derive(Error, Debug)]
pub enum QueueError {
    #[error("failed to acquire from semaphore")]
    Acquire(#[from] AcquireError),
    #[error("failed to join task")]
    Join(#[from] JoinError),
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::num::NonZeroUsize;
    use std::time::{Duration, Instant};

    use anyhow::Result;

    use crate::{create_queue, Task};

    #[tokio::test]
    async fn test() -> Result<()> {
        let result = vec![1, 2, 3, 4, 5, 6, 7];
        assert_time(create_tasks(), &result, 1, 200, 215).await?;
        assert_time(create_tasks(), &result, 2, 100, 115).await?;
        assert_time(create_tasks(), &result, 3, 80, 90).await?;
        assert_time(create_tasks(), &result, 4, 70, 80).await?;
        assert_time(create_tasks(), &result, 5, 60, 70).await?;
        assert_time(create_tasks(), &result, 6, 50, 60).await?;
        assert_time(create_tasks(), &result, 7, 50, 60).await?;
        Ok(())
    }

    async fn assert_time<T>(
        tasks: Vec<Task<T>>,
        expected_result: &[T],
        max_workers: usize,
        min_time: u64,
        max_time: u64,
    ) -> Result<()>
    where
        T: Debug + PartialEq + Send + 'static,
    {
        let start = Instant::now();
        let result = create_queue(tasks, NonZeroUsize::try_from(max_workers)?).await?;
        let elapsed = start.elapsed();
        assert_eq!(expected_result, result);
        assert!(elapsed >= Duration::from_millis(min_time));
        assert!(elapsed <= Duration::from_millis(max_time));
        Ok(())
    }

    fn create_tasks() -> Vec<Task<usize>> {
        vec![
            create_task(1, 10),
            create_task(2, 50),
            create_task(3, 20),
            create_task(4, 30),
            create_task(5, 20),
            create_task(6, 30),
            create_task(7, 40),
        ]
    }

    fn create_task<T>(result: T, time: u64) -> Task<T>
    where
        T: Send + 'static,
    {
        Box::new(move || {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(time)).await;
                result
            })
        })
    }
}
