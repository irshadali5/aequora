use serde::{Deserialize, Serialize};

use super::profile::PolicyError;

/// Hard client-side buffer, batch, worker, and view bounds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientMemoryLimits {
    pub sync_decode_bytes: u32,
    pub sync_response_bytes: u32,
    pub snapshot_pipeline_bytes: u32,
    pub snapshot_chunk_bytes: u32,
    pub max_batch_operations: u32,
    pub max_query_page_rows: u32,
    pub max_parallel_transfers: u8,
    pub max_cpu_workers: u8,
    pub diagnostic_events: u16,
}

impl ClientMemoryLimits {
    pub(crate) const fn validate(self) -> Result<(), PolicyError> {
        if self.sync_decode_bytes == 0
            || self.sync_response_bytes == 0
            || self.snapshot_pipeline_bytes == 0
            || self.snapshot_chunk_bytes == 0
            || self.snapshot_chunk_bytes > self.snapshot_pipeline_bytes
            || self.max_batch_operations == 0
            || self.max_query_page_rows == 0
            || self.max_parallel_transfers == 0
            || self.max_cpu_workers == 0
            || self.diagnostic_events == 0
        {
            return Err(PolicyError::InvalidMemoryLimits);
        }
        Ok(())
    }
}
