//! Validated, secret-free RON configuration for Aequora runtime components.

use aequora_client::{AdaptiveBatchConfig, ClientConfig, RetryConfig, SyncCoordinatorConfig};
use aequora_compute::ComputeConfig;
use aequora_protocol::{Capability, ClientLimits, SessionMetadata, SnapshotLimits};
use aequora_server::ServerConfig;
use aequora_types::ProtocolVersion;
use aequora_validator::ProtocolLimits;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[cfg(feature = "axum")]
use aequora_axum::AxumConfig;
#[cfg(feature = "http-client")]
use aequora_http::HttpTransportConfig;
#[cfg(feature = "quic")]
use aequora_quic::QuicConfig;

/// Complete runtime tuning configuration. Authentication credentials and database URLs are
/// deliberately owned by the host application and do not belong in this structure.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AequoraConfig {
    /// Wire protocol compatibility.
    pub protocol: ProtocolConfig,
    /// Client push and server validation bounds.
    pub push: PushConfig,
    /// Server pull and client response bounds.
    pub pull: PullConfig,
    /// Retry and drain controls.
    pub retry: RetryPolicyConfig,
    /// Dedicated CPU pool settings.
    pub compute: ComputePoolConfig,
    /// Negotiated compression settings.
    pub compression: CompressionConfig,
    /// Untrusted-input and snapshot bounds.
    pub limits: ResourceLimitsConfig,
    /// Background synchronization settings.
    pub coordinator: CoordinatorConfig,
    /// Production server admission, deadline, and readiness controls.
    pub operational: OperationalConfig,
}

/// Wire protocol selection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProtocolConfig {
    /// Oldest wire protocol accepted during a rolling upgrade.
    pub minimum_version: u16,
    /// Supported wire protocol number.
    pub version: u16,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            minimum_version: 1,
            version: 1,
        }
    }
}

/// Outgoing operation-batch controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PushConfig {
    /// Maximum operations in one exchange.
    pub max_operations: usize,
    /// Maximum compressed HTTP request body.
    pub max_bytes: usize,
    /// Maximum local-mutation batching delay in milliseconds. Zero disables debouncing.
    pub max_wait_ms: u64,
    /// Optional latency-driven operation-count tuning.
    pub adaptive: Option<AdaptivePushConfig>,
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            max_operations: 256,
            max_bytes: 1_024 * 1_024,
            max_wait_ms: 100,
            adaptive: None,
        }
    }
}

/// Deterministic additive-increase/multiplicative-decrease push tuning.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptivePushConfig {
    /// Smallest operation count after congestion.
    pub minimum_operations: usize,
    /// Hard operation-count ceiling.
    pub maximum_operations: usize,
    /// Additive growth after a fast successful exchange.
    pub increase_step: usize,
    /// Latency at or below which a complete batch grows.
    pub target_latency_ms: u64,
}

/// Incoming authoritative journal-page controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PullConfig {
    /// Maximum changes accepted in one response.
    pub max_events: u32,
    /// Maximum response bytes accumulated from HTTP.
    pub max_bytes: usize,
}

impl Default for PullConfig {
    fn default() -> Self {
        Self {
            max_events: 1_024,
            max_bytes: 4 * 1_024 * 1_024,
        }
    }
}

/// Retry and per-drain safety controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryPolicyConfig {
    /// Total attempts including the initial request.
    pub max_attempts: u32,
    /// Initial retry delay in milliseconds.
    pub initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    pub max_ms: u64,
    /// Integer exponential multiplier.
    pub multiplier: u32,
    /// Symmetric jitter percentage.
    pub jitter_percent: u8,
    /// Maximum exchanges in one complete synchronization drain.
    pub max_exchanges_per_sync: usize,
}

impl Default for RetryPolicyConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_ms: 500,
            max_ms: 30_000,
            multiplier: 2,
            jitter_percent: 20,
            max_exchanges_per_sync: 1_024,
        }
    }
}

/// Dedicated Rayon pool configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ComputePoolConfig {
    /// Worker threads reserved for synchronization CPU work.
    pub worker_threads: usize,
    /// Item threshold for offloading parallel work.
    pub parallel_threshold: usize,
}

impl Default for ComputePoolConfig {
    fn default() -> Self {
        Self {
            worker_threads: 4,
            parallel_threshold: 128,
        }
    }
}

/// Compression algorithm selection.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompressionAlgorithm {
    /// Disable frame compression.
    None,
    /// Negotiate Zstandard frame compression.
    #[default]
    Zstd,
}

/// Negotiated response compression settings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompressionConfig {
    /// Compression algorithm offered by the client.
    pub algorithm: CompressionAlgorithm,
    /// Minimum serialized bytes before compression is attempted.
    pub min_bytes: usize,
    /// Zstandard compression level used by the Axum boundary.
    pub zstd_level: i32,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Zstd,
            min_bytes: 4_096,
            zstd_level: 3,
        }
    }
}

/// Bounds applied before domain execution or snapshot reconciliation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceLimitsConfig {
    /// Maximum bytes in one operation payload.
    pub max_operation_bytes: usize,
    /// Maximum operation dependencies.
    pub max_dependencies: usize,
    /// Maximum trace identifier bytes.
    pub max_trace_id_bytes: usize,
    /// Maximum partial-scope selectors.
    pub max_partitions: usize,
    /// Maximum bytes in one selector.
    pub max_partition_bytes: usize,
    /// Maximum bytes after frame decompression.
    pub max_decompressed_bytes: usize,
    /// Maximum entities in one snapshot page.
    pub max_snapshot_entities: u32,
    /// Maximum application payload bytes in one snapshot page.
    pub max_snapshot_bytes: u32,
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            max_operation_bytes: 256 * 1_024,
            max_dependencies: 32,
            max_trace_id_bytes: 128,
            max_partitions: 32,
            max_partition_bytes: 1_024,
            max_decompressed_bytes: 4 * 1_024 * 1_024,
            max_snapshot_entities: 512,
            max_snapshot_bytes: 4 * 1_024 * 1_024,
        }
    }
}

/// Background worker controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoordinatorConfig {
    /// Maximum queued wake signals.
    pub channel_capacity: usize,
    /// Periodic wake interval in milliseconds. `None` disables the timer.
    pub periodic_interval_ms: Option<u64>,
    /// Drain once immediately after the coordinator starts.
    pub sync_on_start: bool,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 32,
            periodic_interval_ms: Some(30_000),
            sync_on_start: false,
        }
    }
}

/// Production HTTP trust-boundary controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct OperationalConfig {
    /// Maximum admitted exchange and bootstrap handlers.
    pub max_in_flight_requests: usize,
    /// Maximum admitted exchange and bootstrap handlers for one authenticated tenant.
    pub max_in_flight_per_tenant: usize,
    /// Sustained admitted requests per second for one authenticated tenant.
    pub tenant_requests_per_second: u32,
    /// Maximum immediately consumable request tokens for one authenticated tenant.
    pub tenant_request_burst: u32,
    /// Maximum tenant rate buckets retained by one server process.
    pub max_rate_limit_tenants: usize,
    /// Inactive tenant bucket retention in milliseconds.
    pub rate_limit_idle_timeout_ms: u64,
    /// Complete compressed request-body receive deadline in milliseconds.
    pub body_read_timeout_ms: u64,
    /// Authoritative service execution deadline in milliseconds.
    pub request_timeout_ms: u64,
    /// Dependency-readiness deadline in milliseconds.
    pub readiness_timeout_ms: u64,
    /// Maximum graceful-drain wait in milliseconds.
    pub drain_timeout_ms: u64,
    /// Whole seconds advertised to clients after overload or deadline rejection.
    pub retry_after_seconds: u64,
}

impl Default for OperationalConfig {
    fn default() -> Self {
        Self {
            max_in_flight_requests: 256,
            max_in_flight_per_tenant: 64,
            tenant_requests_per_second: 64,
            tenant_request_burst: 128,
            max_rate_limit_tenants: 4_096,
            rate_limit_idle_timeout_ms: 300_000,
            body_read_timeout_ms: 15_000,
            request_timeout_ms: 30_000,
            readiness_timeout_ms: 2_000,
            drain_timeout_ms: 30_000,
            retry_after_seconds: 1,
        }
    }
}

/// Configuration parsing or validation failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    /// RON could not be decoded into the strict configuration schema.
    #[error("invalid Aequora RON configuration: {0}")]
    Ron(String),
    /// One field or cross-field relationship is unsafe.
    #[error("invalid Aequora configuration: {0}")]
    Invalid(&'static str),
}

impl AequoraConfig {
    /// Parses and validates a strict RON configuration. Unknown fields are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for malformed RON, unknown fields, or unsafe values.
    pub fn from_ron(input: &str) -> Result<Self, ConfigError> {
        let config: Self =
            ron::from_str(input).map_err(|error| ConfigError::Ron(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Validates every non-zero bound and important cross-field relationship.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Invalid`] for unsupported or unsafe settings.
    pub const fn validate(&self) -> Result<(), ConfigError> {
        if self.protocol.minimum_version == 0
            || self.protocol.minimum_version > self.protocol.version
        {
            return Err(ConfigError::Invalid(
                "protocol compatibility window is inconsistent",
            ));
        }
        if self.push.max_operations == 0 || self.push.max_bytes == 0 {
            return Err(ConfigError::Invalid(
                "push limits must be greater than zero",
            ));
        }
        if let Some(adaptive) = self.push.adaptive {
            if adaptive.minimum_operations == 0
                || adaptive.maximum_operations < adaptive.minimum_operations
                || adaptive.increase_step == 0
                || adaptive.target_latency_ms == 0
            {
                return Err(ConfigError::Invalid(
                    "adaptive push settings are inconsistent",
                ));
            }
        }
        if self.pull.max_events == 0 || self.pull.max_bytes == 0 {
            return Err(ConfigError::Invalid(
                "pull limits must be greater than zero",
            ));
        }
        if self.pull.max_bytes > u32::MAX as usize {
            return Err(ConfigError::Invalid(
                "pull max_bytes must fit the wire protocol",
            ));
        }
        if self.retry.max_attempts == 0
            || self.retry.initial_ms > self.retry.max_ms
            || self.retry.multiplier == 0
            || self.retry.jitter_percent > 100
            || self.retry.max_exchanges_per_sync == 0
        {
            return Err(ConfigError::Invalid("retry settings are inconsistent"));
        }
        if self.compute.worker_threads == 0 || self.compute.parallel_threshold == 0 {
            return Err(ConfigError::Invalid(
                "compute limits must be greater than zero",
            ));
        }
        if self.compression.min_bytes == 0 {
            return Err(ConfigError::Invalid(
                "compression min_bytes must be greater than zero",
            ));
        }
        if self.limits.max_operation_bytes == 0
            || self.limits.max_dependencies == 0
            || self.limits.max_trace_id_bytes == 0
            || self.limits.max_partitions == 0
            || self.limits.max_partition_bytes == 0
            || self.limits.max_decompressed_bytes < self.push.max_bytes
            || self.limits.max_decompressed_bytes < self.pull.max_bytes
            || self.limits.max_snapshot_entities == 0
            || self.limits.max_snapshot_bytes == 0
        {
            return Err(ConfigError::Invalid("resource limits are inconsistent"));
        }
        if self.coordinator.channel_capacity == 0
            || matches!(self.coordinator.periodic_interval_ms, Some(0))
        {
            return Err(ConfigError::Invalid("coordinator limits are inconsistent"));
        }
        if self.operational.max_in_flight_requests == 0
            || self.operational.max_in_flight_per_tenant == 0
            || self.operational.tenant_requests_per_second == 0
            || self.operational.tenant_request_burst == 0
            || self.operational.max_rate_limit_tenants == 0
            || self.operational.rate_limit_idle_timeout_ms == 0
            || self.operational.body_read_timeout_ms == 0
            || self.operational.request_timeout_ms == 0
            || self.operational.readiness_timeout_ms == 0
            || self.operational.drain_timeout_ms == 0
            || self.operational.retry_after_seconds == 0
        {
            return Err(ConfigError::Invalid(
                "operational limits must be greater than zero",
            ));
        }
        if self.operational.max_in_flight_per_tenant > self.operational.max_in_flight_requests {
            return Err(ConfigError::Invalid(
                "per-tenant admission limit must not exceed the global limit",
            ));
        }
        if self.operational.max_rate_limit_tenants < self.operational.max_in_flight_requests {
            return Err(ConfigError::Invalid(
                "rate-limit tenant capacity must cover the global in-flight limit",
            ));
        }
        Ok(())
    }

    /// Produces client engine settings for an authenticated session.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when this configuration is invalid.
    pub fn client_config(&self, session: SessionMetadata) -> Result<ClientConfig, ConfigError> {
        self.validate()?;
        let mut config = ClientConfig::new(session);
        config.protocol = ProtocolVersion(self.protocol.version);
        config.push_batch_size = self.push.max_operations;
        config.push_batch_bytes = self.push.max_bytes;
        config.adaptive_batching = self.push.adaptive.map(|adaptive| AdaptiveBatchConfig {
            minimum_operations: adaptive.minimum_operations,
            maximum_operations: adaptive.maximum_operations,
            increase_step: adaptive.increase_step,
            target_latency: Duration::from_millis(adaptive.target_latency_ms),
        });
        config.limits = ClientLimits {
            max_changes: self.pull.max_events,
            max_response_bytes: u32::try_from(self.pull.max_bytes)
                .map_err(|_| ConfigError::Invalid("pull max_bytes must fit the wire protocol"))?,
        };
        config.retry = RetryConfig {
            max_attempts: self.retry.max_attempts,
            initial_delay: Duration::from_millis(self.retry.initial_ms),
            max_delay: Duration::from_millis(self.retry.max_ms),
            multiplier: self.retry.multiplier,
            jitter_percent: self.retry.jitter_percent,
        };
        config.max_exchanges_per_sync = self.retry.max_exchanges_per_sync;
        config.snapshot_limits = SnapshotLimits {
            max_entities: self.limits.max_snapshot_entities,
            max_payload_bytes: self.limits.max_snapshot_bytes,
        };
        if self.compression.algorithm == CompressionAlgorithm::Zstd {
            config.capabilities.push(Capability::Zstd);
        }
        Ok(config)
    }

    /// Produces authoritative server validation and pull settings.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when this configuration is invalid.
    pub fn server_config(&self) -> Result<ServerConfig, ConfigError> {
        self.validate()?;
        Ok(ServerConfig {
            limits: ProtocolLimits {
                minimum_protocol: ProtocolVersion(self.protocol.minimum_version),
                current_protocol: ProtocolVersion(self.protocol.version),
                max_operations: self.push.max_operations,
                max_operation_bytes: self.limits.max_operation_bytes,
                max_dependencies: self.limits.max_dependencies,
                max_trace_id_bytes: self.limits.max_trace_id_bytes,
                max_partitions: self.limits.max_partitions,
                max_partition_bytes: self.limits.max_partition_bytes,
            },
            max_pull_changes: usize::try_from(self.pull.max_events).unwrap_or(usize::MAX),
        })
    }

    /// Produces HTTP server framing limits.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when this configuration is invalid.
    #[cfg(feature = "axum")]
    pub fn axum_config(&self) -> Result<AxumConfig, ConfigError> {
        self.validate()?;
        Ok(AxumConfig {
            max_body_bytes: self.push.max_bytes,
            max_decompressed_bytes: self.limits.max_decompressed_bytes,
            body_read_timeout: Duration::from_millis(self.operational.body_read_timeout_ms),
            compression_threshold: self.compression.min_bytes,
            zstd_level: self.compression.zstd_level,
            zstd_enabled: self.compression.algorithm == CompressionAlgorithm::Zstd,
            max_in_flight_requests: self.operational.max_in_flight_requests,
            max_in_flight_per_tenant: self.operational.max_in_flight_per_tenant,
            tenant_requests_per_second: self.operational.tenant_requests_per_second,
            tenant_request_burst: self.operational.tenant_request_burst,
            max_rate_limit_tenants: self.operational.max_rate_limit_tenants,
            rate_limit_idle_timeout: Duration::from_millis(
                self.operational.rate_limit_idle_timeout_ms,
            ),
            request_timeout: Duration::from_millis(self.operational.request_timeout_ms),
            readiness_timeout: Duration::from_millis(self.operational.readiness_timeout_ms),
            drain_timeout: Duration::from_millis(self.operational.drain_timeout_ms),
            retry_after_seconds: self.operational.retry_after_seconds,
        })
    }

    /// Produces HTTP client response limits.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when this configuration is invalid.
    #[cfg(feature = "http-client")]
    pub fn http_transport_config(&self) -> Result<HttpTransportConfig, ConfigError> {
        self.validate()?;
        Ok(HttpTransportConfig {
            max_response_bytes: self.pull.max_bytes,
            max_decompressed_response_bytes: self.limits.max_decompressed_bytes,
            compression_threshold: self.compression.min_bytes,
            request_zstd_level: (self.compression.algorithm == CompressionAlgorithm::Zstd)
                .then_some(self.compression.zstd_level),
        })
    }

    /// Produces symmetric QUIC framing, decompression, and compression settings.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when this configuration is invalid.
    #[cfg(feature = "quic")]
    pub fn quic_config(&self) -> Result<QuicConfig, ConfigError> {
        self.validate()?;
        Ok(QuicConfig {
            max_request_bytes: self.push.max_bytes,
            max_decompressed_request_bytes: self.limits.max_decompressed_bytes,
            max_response_bytes: self.pull.max_bytes,
            max_decompressed_response_bytes: self.limits.max_decompressed_bytes,
            compression_threshold: self.compression.min_bytes,
            zstd_level: self.compression.zstd_level,
            zstd_enabled: self.compression.algorithm == CompressionAlgorithm::Zstd,
        })
    }

    /// Produces dedicated CPU-pool settings.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when this configuration is invalid.
    pub fn compute_config(&self) -> Result<ComputeConfig, ConfigError> {
        self.validate()?;
        Ok(ComputeConfig {
            worker_threads: self.compute.worker_threads,
            parallel_threshold: self.compute.parallel_threshold,
        })
    }

    /// Produces background coordinator settings.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when this configuration is invalid.
    pub fn coordinator_config(&self) -> Result<SyncCoordinatorConfig, ConfigError> {
        self.validate()?;
        Ok(SyncCoordinatorConfig {
            channel_capacity: self.coordinator.channel_capacity,
            periodic_interval: self
                .coordinator
                .periodic_interval_ms
                .map(Duration::from_millis),
            sync_on_start: self.coordinator.sync_on_start,
            mutation_debounce: Duration::from_millis(self.push.max_wait_ms),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{ActorId, DeviceId, SessionId, SyncScopeId, TenantId};

    fn session() -> SessionMetadata {
        SessionMetadata {
            session_id: SessionId::new(),
            device_id: DeviceId::new(),
            actor_id: ActorId::new(),
            tenant_id: TenantId::new(),
            scope_id: SyncScopeId::new(),
            partitions: Vec::new(),
        }
    }

    #[test]
    fn partial_ron_uses_checked_defaults_and_maps_consistently() {
        let config = AequoraConfig::from_ron(
            "(push: (max_operations: 64, adaptive: Some((minimum_operations: 8, maximum_operations: 128, increase_step: 8, target_latency_ms: 100))), coordinator: (periodic_interval_ms: None))",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let client = config
            .client_config(session())
            .unwrap_or_else(|error| panic!("{error}"));
        let server = config
            .server_config()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(client.push_batch_size, 64);
        assert_eq!(client.push_batch_bytes, 1_024 * 1_024);
        assert_eq!(
            client.adaptive_batching,
            Some(AdaptiveBatchConfig {
                minimum_operations: 8,
                maximum_operations: 128,
                increase_step: 8,
                target_latency: Duration::from_millis(100),
            })
        );
        assert_eq!(server.limits.max_operations, 64);
        assert!(client.capabilities.contains(&Capability::Zstd));
        assert_eq!(
            config
                .coordinator_config()
                .unwrap_or_else(|error| panic!("{error}"))
                .periodic_interval,
            None
        );
    }

    #[test]
    fn unknown_and_cross_field_unsafe_values_are_rejected() {
        assert!(AequoraConfig::from_ron("(unknown: 1)").is_err());
        assert!(
            AequoraConfig::from_ron(
                "(pull: (max_bytes: 8388608), limits: (max_decompressed_bytes: 4194304))"
            )
            .is_err()
        );
        assert!(AequoraConfig::from_ron("(operational: (max_in_flight_requests: 0))").is_err());
        assert!(AequoraConfig::from_ron("(operational: (max_in_flight_per_tenant: 0))").is_err());
        assert!(AequoraConfig::from_ron("(operational: (tenant_requests_per_second: 0))").is_err());
        assert!(AequoraConfig::from_ron("(operational: (tenant_request_burst: 0))").is_err());
        assert!(AequoraConfig::from_ron("(operational: (max_rate_limit_tenants: 0))").is_err());
        assert!(AequoraConfig::from_ron("(operational: (rate_limit_idle_timeout_ms: 0))").is_err());
        assert!(AequoraConfig::from_ron("(operational: (body_read_timeout_ms: 0))").is_err());
        assert!(
            AequoraConfig::from_ron(
                "(operational: (max_in_flight_requests: 4, max_in_flight_per_tenant: 5))"
            )
            .is_err()
        );
        assert!(
            AequoraConfig::from_ron(
                "(operational: (max_in_flight_requests: 8, max_rate_limit_tenants: 7))"
            )
            .is_err()
        );
        let mut config = AequoraConfig::default();
        config.operational.request_timeout_ms = 0;
        assert!(config.validate().is_err());
        config = AequoraConfig::default();
        config.operational.readiness_timeout_ms = 0;
        assert!(config.validate().is_err());
        config = AequoraConfig::default();
        config.operational.retry_after_seconds = 0;
        assert!(config.validate().is_err());
        config = AequoraConfig::default();
        config.operational.drain_timeout_ms = 0;
        assert!(config.validate().is_err());
    }

    #[cfg(feature = "axum")]
    #[test]
    fn operational_limits_reach_the_axum_boundary() {
        let config = AequoraConfig::from_ron(
            "(operational: (max_in_flight_requests: 7, max_in_flight_per_tenant: 3, tenant_requests_per_second: 11, tenant_request_burst: 13, max_rate_limit_tenants: 17, rate_limit_idle_timeout_ms: 19000, body_read_timeout_ms: 5000, request_timeout_ms: 9000, readiness_timeout_ms: 700, drain_timeout_ms: 12000, retry_after_seconds: 3))",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let axum = config
            .axum_config()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(axum.max_in_flight_requests, 7);
        assert_eq!(axum.max_in_flight_per_tenant, 3);
        assert_eq!(axum.tenant_requests_per_second, 11);
        assert_eq!(axum.tenant_request_burst, 13);
        assert_eq!(axum.max_rate_limit_tenants, 17);
        assert_eq!(axum.rate_limit_idle_timeout, Duration::from_secs(19));
        assert_eq!(axum.body_read_timeout, Duration::from_secs(5));
        assert_eq!(axum.request_timeout, Duration::from_secs(9));
        assert_eq!(axum.readiness_timeout, Duration::from_millis(700));
        assert_eq!(axum.drain_timeout, Duration::from_secs(12));
        assert_eq!(axum.retry_after_seconds, 3);
    }

    #[test]
    fn disabling_compression_reaches_every_enabled_transport_boundary() {
        let mut config = AequoraConfig::default();
        config.compression.algorithm = CompressionAlgorithm::None;
        let client = config
            .client_config(session())
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!client.capabilities.contains(&Capability::Zstd));
        #[cfg(feature = "axum")]
        assert!(
            !config
                .axum_config()
                .unwrap_or_else(|error| panic!("{error}"))
                .zstd_enabled
        );
        #[cfg(feature = "http-client")]
        assert_eq!(
            config
                .http_transport_config()
                .unwrap_or_else(|error| panic!("{error}"))
                .request_zstd_level,
            None
        );
        #[cfg(feature = "quic")]
        assert!(
            !config
                .quic_config()
                .unwrap_or_else(|error| panic!("{error}"))
                .zstd_enabled
        );
    }
}
