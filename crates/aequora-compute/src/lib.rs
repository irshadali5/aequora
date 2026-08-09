//! Dedicated Rayon execution boundary for CPU-heavy work initiated by Tokio tasks.

use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::oneshot;

/// Dedicated compute-pool settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputeConfig {
    /// Worker threads reserved for synchronization CPU work.
    pub worker_threads: usize,
    /// Item count below which sequential execution avoids scheduling overhead.
    pub parallel_threshold: usize,
}

impl Default for ComputeConfig {
    fn default() -> Self {
        Self {
            worker_threads: 4,
            parallel_threshold: 128,
        }
    }
}

/// Failure to construct or execute work on the dedicated pool.
#[derive(Debug, Error)]
pub enum ComputeError {
    /// Thread count must be non-zero.
    #[error("compute worker thread count must be greater than zero")]
    ZeroWorkers,
    /// Rayon could not construct the pool.
    #[error("failed to construct compute pool: {0}")]
    Build(#[from] rayon::ThreadPoolBuildError),
    /// A scheduled job panicked or was otherwise abandoned.
    #[error("compute job did not return a result")]
    WorkerAbandoned,
}

/// Cloneable handle to a dedicated Rayon pool.
#[derive(Clone)]
pub struct ComputePool {
    pool: Arc<ThreadPool>,
    parallel_threshold: usize,
}

impl ComputePool {
    /// Constructs a dedicated pool.
    ///
    /// # Errors
    ///
    /// Returns [`ComputeError`] for a zero worker count or thread-pool construction failure.
    pub fn new(config: ComputeConfig) -> Result<Self, ComputeError> {
        if config.worker_threads == 0 {
            return Err(ComputeError::ZeroWorkers);
        }
        let pool = ThreadPoolBuilder::new()
            .num_threads(config.worker_threads)
            .thread_name(|index| format!("aequora-compute-{index}"))
            .build()?;
        Ok(Self {
            pool: Arc::new(pool),
            parallel_threshold: config.parallel_threshold.max(1),
        })
    }

    /// Returns whether an input is large enough to justify parallel scheduling.
    #[must_use]
    pub const fn should_parallelize(&self, item_count: usize) -> bool {
        item_count >= self.parallel_threshold
    }

    /// Runs one CPU-heavy job without blocking the async runtime's worker thread.
    ///
    /// # Errors
    ///
    /// Returns [`ComputeError::WorkerAbandoned`] if the job panics before sending its result.
    pub async fn run<F, R>(&self, job: F) -> Result<R, ComputeError>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        self.pool.spawn(move || {
            let result = job();
            let _ignored = sender.send(result);
        });
        receiver.await.map_err(|_| ComputeError::WorkerAbandoned)
    }

    /// Maps a large owned input in parallel, preserving its original order.
    /// Small inputs run sequentially to avoid Rayon overhead.
    ///
    /// # Errors
    ///
    /// Returns [`ComputeError::WorkerAbandoned`] if parallel work does not return.
    pub async fn map<T, R, F>(&self, input: Vec<T>, transform: F) -> Result<Vec<R>, ComputeError>
    where
        T: Send + 'static,
        R: Send + 'static,
        F: Fn(T) -> R + Send + Sync + 'static,
    {
        if !self.should_parallelize(input.len()) {
            return Ok(input.into_iter().map(transform).collect());
        }
        self.run(move || input.into_par_iter().map(transform).collect())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parallel_map_preserves_order() {
        let pool = ComputePool::new(ComputeConfig {
            worker_threads: 2,
            parallel_threshold: 2,
        })
        .unwrap_or_else(|error| panic!("{error}"));
        let output = pool
            .map(vec![1, 2, 3], |value| value * value)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(output, vec![1, 4, 9]);
    }
}
