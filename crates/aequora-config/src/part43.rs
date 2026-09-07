//! Part 43 deployment configuration and deterministic source composition.

use aequora_feature_flags::{FeatureDefinition, FeatureError, FeaturePolicy};
use aequora_policy::{
    ChangeClass, ConfigDigest, ConfigGeneration, MutabilityClass, PolicyError, RuntimePolicy,
    diff_runtime_policy,
};
use aequora_secrets::SecretRef;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};
use thiserror::Error;

/// Current deployment configuration schema, independent of protocol and storage versions.
pub const CURRENT_CONFIG_SCHEMA_VERSION: ConfigSchemaVersion = ConfigSchemaVersion(1);

/// Version of the human-edited configuration schema.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ConfigSchemaVersion(pub u16);

/// Closed set of deployment environments.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum Environment {
    Development,
    Test,
    Staging,
    #[default]
    Production,
}

impl FromStr for Environment {
    type Err = ConfigurationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "development" | "dev" => Ok(Self::Development),
            "test" => Ok(Self::Test),
            "staging" => Ok(Self::Staging),
            "production" | "prod" => Ok(Self::Production),
            _ => Err(ConfigurationError::InvalidValue("environment")),
        }
    }
}

/// Explicit TLS deployment mode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TlsMode {
    DisabledDevelopmentOnly,
    DirectTls {
        certificate: SecretRef,
        private_key: SecretRef,
    },
    BehindTrustedProxy,
}

/// Process listener and ingress trust policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpDeploymentConfig {
    pub bind: String,
    pub trusted_proxies: BTreeSet<String>,
    pub tls: TlsMode,
}

impl Default for HttpDeploymentConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8443".to_owned(),
            trusted_proxies: BTreeSet::new(),
            tls: TlsMode::DirectTls {
                certificate: SecretRef::Environment(
                    aequora_secrets::SecretKey::new("AEQUORA_TLS_CERT")
                        .unwrap_or_else(|error| panic!("{error}")),
                ),
                private_key: SecretRef::Environment(
                    aequora_secrets::SecretKey::new("AEQUORA_TLS_PRIVATE_KEY")
                        .unwrap_or_else(|error| panic!("{error}")),
                ),
            },
        }
    }
}

/// Facilities that are categorically rejected outside development/test builds and profiles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DevelopmentFacility {
    AuthenticationBypass,
    DestructiveReset,
    FaultInjection,
    VerbosePayloadLogging,
}

/// Explicit set of enabled development-only facilities.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DevelopmentFacilities(pub BTreeSet<DevelopmentFacility>);

impl DevelopmentFacilities {
    fn any_enabled(&self) -> bool {
        !self.0.is_empty()
    }
}

/// Logical authority adapter selection. Passwords and URLs remain secret references.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthorityAdapterConfig {
    Postgres {
        maximum_connections: u16,
        credential: SecretRef,
    },
    Neon {
        maximum_connections: u16,
        credential: SecretRef,
        cold_start_timeout_ms: u64,
    },
}

impl Default for AuthorityAdapterConfig {
    fn default() -> Self {
        Self::Postgres {
            maximum_connections: 32,
            credential: SecretRef::Environment(
                aequora_secrets::SecretKey::new("AEQUORA_DATABASE_URL")
                    .unwrap_or_else(|error| panic!("{error}")),
            ),
        }
    }
}

/// Logical local adapter selection. Physical adapter knobs stay in their owning crates.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum LocalAdapterConfig {
    #[default]
    Sqlite,
    Stoolap,
}

/// Certified capabilities supplied by adapter composition rather than guessed from config.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AdapterCapabilities {
    pub authoritative_transactions: bool,
    pub local_atomicity: bool,
    pub snapshot_generation_swap: bool,
}

/// Strongly typed, complete deployment configuration before capability validation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawDeploymentConfig {
    pub schema_version: ConfigSchemaVersion,
    pub environment: Environment,
    pub http: HttpDeploymentConfig,
    pub authority: AuthorityAdapterConfig,
    pub local_store: LocalAdapterConfig,
    pub runtime: RuntimePolicy,
    pub development: DevelopmentFacilities,
    pub require_snapshot_generation_swap: bool,
    pub features: Vec<FeatureDefinition>,
}

impl Default for RawDeploymentConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_CONFIG_SCHEMA_VERSION,
            environment: Environment::Production,
            http: HttpDeploymentConfig::default(),
            authority: AuthorityAdapterConfig::default(),
            local_store: LocalAdapterConfig::default(),
            runtime: RuntimePolicy::default(),
            development: DevelopmentFacilities::default(),
            require_snapshot_generation_swap: false,
            features: Vec::new(),
        }
    }
}

/// Field-level override used by one deterministic source layer.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigPatch {
    pub schema_version: Option<ConfigSchemaVersion>,
    pub environment: Option<Environment>,
    pub http: Option<HttpDeploymentConfig>,
    pub authority: Option<AuthorityAdapterConfig>,
    pub local_store: Option<LocalAdapterConfig>,
    pub runtime: Option<RuntimePolicy>,
    pub development: Option<DevelopmentFacilities>,
    pub require_snapshot_generation_swap: Option<bool>,
    pub features: Option<Vec<FeatureDefinition>>,
}

impl ConfigPatch {
    fn apply(
        self,
        target: &mut RawDeploymentConfig,
        source: &ConfigSource,
        provenance: &mut BTreeMap<String, ConfigSource>,
    ) {
        macro_rules! apply {
            ($field:ident, $name:literal) => {
                if let Some(value) = self.$field {
                    target.$field = value;
                    provenance.insert($name.to_owned(), source.clone());
                }
            };
        }
        apply!(schema_version, "schema_version");
        apply!(environment, "environment");
        apply!(http, "http");
        apply!(authority, "authority");
        apply!(local_store, "local_store");
        apply!(runtime, "runtime");
        apply!(development, "development");
        apply!(
            require_snapshot_generation_swap,
            "require_snapshot_generation_swap"
        );
        apply!(features, "features");
    }
}

/// Non-secret origin of an effective setting.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConfigSource {
    CompiledDefault,
    BaseRon(String),
    EnvironmentRon(String),
    EnvironmentVariable(String),
    CommandLine,
    RuntimePolicy,
}

/// Parsed, capability-validated configuration. Only this typestate can become effective.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedDeploymentConfig(RawDeploymentConfig);

impl ValidatedDeploymentConfig {
    /// Promotes validated configuration to one immutable effective generation.
    ///
    /// # Errors
    ///
    /// Returns an encoding or feature-policy error.
    pub fn effective(
        self,
        generation: ConfigGeneration,
        provenance: BTreeMap<String, ConfigSource>,
    ) -> Result<EffectiveConfig, ConfigurationError> {
        let raw = self.0;
        let digest_bytes = postcard::to_stdvec(&raw)
            .map_err(|error| ConfigurationError::Encoding(error.to_string()))?;
        let digest = ConfigDigest::from_bytes(*blake3::hash(&digest_bytes).as_bytes());
        Ok(EffectiveConfig {
            generation,
            digest,
            environment: raw.environment,
            http: raw.http,
            authority: raw.authority,
            local_store: raw.local_store,
            runtime: raw.runtime,
            features: FeaturePolicy::new(raw.features)?,
            provenance,
        })
    }
}

impl RawDeploymentConfig {
    /// Parses the strict current RON schema and validates all cross-field constraints.
    ///
    /// # Errors
    ///
    /// Returns a syntax, unknown-field, schema, safety, or capability error.
    pub fn from_ron(
        input: &str,
        capabilities: AdapterCapabilities,
    ) -> Result<ValidatedDeploymentConfig, ConfigurationError> {
        let raw: Self =
            ron::from_str(input).map_err(|error| ConfigurationError::Ron(error.to_string()))?;
        raw.validate(capabilities)
    }

    /// Validates schema, bounds, production safety, and adapter capabilities.
    ///
    /// # Errors
    ///
    /// Fails closed for every unsafe or unsupported combination.
    pub fn validate(
        self,
        capabilities: AdapterCapabilities,
    ) -> Result<ValidatedDeploymentConfig, ConfigurationError> {
        if self.schema_version != CURRENT_CONFIG_SCHEMA_VERSION {
            return Err(ConfigurationError::UnsupportedSchema(self.schema_version.0));
        }
        self.runtime.validate()?;
        if self.http.bind.trim().is_empty() {
            return Err(ConfigurationError::InvalidValue("http.bind"));
        }
        if self.http.trusted_proxies.iter().any(|proxy| proxy == "*") {
            return Err(ConfigurationError::InvalidValue("http.trusted_proxies"));
        }
        if matches!(
            self.environment,
            Environment::Production | Environment::Staging
        ) {
            if self.development.any_enabled() {
                return Err(ConfigurationError::ProductionSafety);
            }
            if matches!(self.http.tls, TlsMode::DisabledDevelopmentOnly) {
                return Err(ConfigurationError::ProductionSafety);
            }
        }
        if self.environment == Environment::Test
            && self.development.any_enabled()
            && !cfg!(feature = "test-facilities")
        {
            return Err(ConfigurationError::TestFacilitiesNotCompiled);
        }
        let maximum_connections = match &self.authority {
            AuthorityAdapterConfig::Postgres {
                maximum_connections,
                ..
            }
            | AuthorityAdapterConfig::Neon {
                maximum_connections,
                ..
            } => *maximum_connections,
        };
        if maximum_connections == 0 {
            return Err(ConfigurationError::InvalidValue(
                "authority.maximum_connections",
            ));
        }
        if !capabilities.authoritative_transactions || !capabilities.local_atomicity {
            return Err(ConfigurationError::UnsupportedAdapterCapability);
        }
        if self.require_snapshot_generation_swap && !capabilities.snapshot_generation_swap {
            return Err(ConfigurationError::UnsupportedAdapterCapability);
        }
        FeaturePolicy::new(self.features.clone())?;
        Ok(ValidatedDeploymentConfig(self))
    }
}

/// Fully validated, immutable configuration accepted by a composition root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveConfig {
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub environment: Environment,
    pub http: HttpDeploymentConfig,
    pub authority: AuthorityAdapterConfig,
    pub local_store: LocalAdapterConfig,
    pub runtime: RuntimePolicy,
    pub features: FeaturePolicy,
    pub provenance: BTreeMap<String, ConfigSource>,
}

impl EffectiveConfig {
    /// Returns a secret-redacted RON representation suitable for CLI and incident bundles.
    ///
    /// # Errors
    ///
    /// Returns an encoding error if serialization fails.
    pub fn sanitized_ron(&self) -> Result<String, ConfigurationError> {
        #[derive(Serialize)]
        struct Sanitized<'a> {
            generation: ConfigGeneration,
            digest: String,
            environment: Environment,
            http_bind: &'a str,
            tls: &'static str,
            authority: &'static str,
            local_store: LocalAdapterConfig,
            runtime: &'a RuntimePolicy,
            features: usize,
            secrets: &'static str,
        }
        let tls = match self.http.tls {
            TlsMode::DisabledDevelopmentOnly => "DisabledDevelopmentOnly",
            TlsMode::DirectTls { .. } => "DirectTls(<redacted>)",
            TlsMode::BehindTrustedProxy => "BehindTrustedProxy",
        };
        let authority = match self.authority {
            AuthorityAdapterConfig::Postgres { .. } => "Postgres(<redacted>)",
            AuthorityAdapterConfig::Neon { .. } => "Neon(<redacted>)",
        };
        ron::ser::to_string_pretty(
            &Sanitized {
                generation: self.generation,
                digest: self.digest.to_string(),
                environment: self.environment,
                http_bind: &self.http.bind,
                tls,
                authority,
                local_store: self.local_store.clone(),
                runtime: &self.runtime,
                features: self.features.definitions().count(),
                secrets: "<redacted>",
            },
            ron::ser::PrettyConfig::default(),
        )
        .map_err(|error| ConfigurationError::Encoding(error.to_string()))
    }
}

/// Deterministic source loader. Method call order cannot change source precedence.
pub struct ConfigLoader {
    defaults: RawDeploymentConfig,
    base: Option<(String, ConfigPatch)>,
    environment_file: Option<(String, ConfigPatch)>,
    environment: Option<EnvironmentOverrides>,
    cli: Option<ConfigPatch>,
    runtime: Option<RuntimePolicy>,
}

impl ConfigLoader {
    /// Starts with compiled, strongly typed defaults.
    #[must_use]
    pub fn new(defaults: RawDeploymentConfig) -> Self {
        Self {
            defaults,
            base: None,
            environment_file: None,
            environment: None,
            cli: None,
            runtime: None,
        }
    }

    /// Adds the base RON source.
    ///
    /// # Errors
    ///
    /// Rejects malformed RON and unknown fields.
    pub fn base_ron(
        mut self,
        name: impl Into<String>,
        input: &str,
    ) -> Result<Self, ConfigurationError> {
        self.base = Some((name.into(), parse_patch(input)?));
        Ok(self)
    }

    /// Adds the environment-specific RON source.
    ///
    /// # Errors
    ///
    /// Rejects malformed RON and unknown fields.
    pub fn environment_ron(
        mut self,
        name: impl Into<String>,
        input: &str,
    ) -> Result<Self, ConfigurationError> {
        self.environment_file = Some((name.into(), parse_patch(input)?));
        Ok(self)
    }

    /// Adds supported environment-variable values without reading arbitrary process state.
    ///
    /// # Errors
    ///
    /// Rejects malformed typed values. Unknown `AEQUORA_` keys are rejected to catch typos.
    pub fn environment(
        mut self,
        values: &BTreeMap<String, String>,
    ) -> Result<Self, ConfigurationError> {
        self.environment = Some(environment_patch(values)?);
        Ok(self)
    }

    /// Adds explicit command-line overrides.
    #[must_use]
    pub fn cli(mut self, patch: ConfigPatch) -> Self {
        self.cli = Some(patch);
        self
    }

    /// Adds the normalized dynamic policy source, which has highest precedence.
    #[must_use]
    pub fn runtime_policy(mut self, policy: RuntimePolicy) -> Self {
        self.runtime = Some(policy);
        self
    }

    /// Applies fixed precedence, validates, computes a non-secret semantic digest, and emits one
    /// effective generation.
    ///
    /// # Errors
    ///
    /// Returns parse, validation, capability, feature, policy, or encoding errors.
    pub fn load(
        self,
        generation: ConfigGeneration,
        capabilities: AdapterCapabilities,
    ) -> Result<EffectiveConfig, ConfigurationError> {
        let mut raw = self.defaults;
        let mut provenance = BTreeMap::from([
            ("schema_version".to_owned(), ConfigSource::CompiledDefault),
            ("environment".to_owned(), ConfigSource::CompiledDefault),
            ("http".to_owned(), ConfigSource::CompiledDefault),
            ("authority".to_owned(), ConfigSource::CompiledDefault),
            ("local_store".to_owned(), ConfigSource::CompiledDefault),
            ("runtime".to_owned(), ConfigSource::CompiledDefault),
            ("development".to_owned(), ConfigSource::CompiledDefault),
            ("features".to_owned(), ConfigSource::CompiledDefault),
        ]);
        if let Some((name, patch)) = self.base {
            patch.apply(&mut raw, &ConfigSource::BaseRon(name), &mut provenance);
        }
        if let Some((name, patch)) = self.environment_file {
            patch.apply(
                &mut raw,
                &ConfigSource::EnvironmentRon(name),
                &mut provenance,
            );
        }
        if let Some(patch) = self.environment {
            let source = ConfigSource::EnvironmentVariable("AEQUORA_*".to_owned());
            if let Some(environment) = patch.environment {
                raw.environment = environment;
                provenance.insert("environment".to_owned(), source.clone());
            }
            if let Some(batch_size) = patch.batch_size {
                raw.runtime.batch_size = batch_size;
                provenance.insert("runtime.batch_size".to_owned(), source.clone());
            }
            if let Some(request_timeout) = patch.request_timeout {
                raw.runtime.request_timeout = request_timeout;
                provenance.insert("runtime.request_timeout_ms".to_owned(), source);
            }
        }
        if let Some(patch) = self.cli {
            patch.apply(&mut raw, &ConfigSource::CommandLine, &mut provenance);
        }
        if let Some(runtime) = self.runtime {
            ConfigPatch {
                runtime: Some(runtime),
                ..ConfigPatch::default()
            }
            .apply(&mut raw, &ConfigSource::RuntimePolicy, &mut provenance);
        }
        raw.validate(capabilities)?
            .effective(generation, provenance)
    }
}

fn parse_patch(input: &str) -> Result<ConfigPatch, ConfigurationError> {
    ron::from_str(input).map_err(|error| ConfigurationError::Ron(error.to_string()))
}

#[derive(Default)]
struct EnvironmentOverrides {
    environment: Option<Environment>,
    batch_size: Option<aequora_policy::BatchSize>,
    request_timeout: Option<aequora_policy::TimeoutMillis>,
}

fn environment_patch(
    values: &BTreeMap<String, String>,
) -> Result<EnvironmentOverrides, ConfigurationError> {
    let mut patch = EnvironmentOverrides::default();
    for (key, value) in values {
        match key.as_str() {
            "AEQUORA_ENVIRONMENT" => patch.environment = Some(value.parse()?),
            "AEQUORA_BATCH_SIZE" => {
                patch.batch_size =
                    Some(aequora_policy::BatchSize::new(value.parse().map_err(
                        |_| ConfigurationError::InvalidValue("AEQUORA_BATCH_SIZE"),
                    )?)?);
            }
            "AEQUORA_REQUEST_TIMEOUT_MS" => {
                patch.request_timeout =
                    Some(aequora_policy::TimeoutMillis::new(value.parse().map_err(
                        |_| ConfigurationError::InvalidValue("AEQUORA_REQUEST_TIMEOUT_MS"),
                    )?)?);
            }
            key if key.starts_with("AEQUORA_") => {
                return Err(ConfigurationError::UnknownEnvironmentKey(key.to_owned()));
            }
            _ => {}
        }
    }
    Ok(patch)
}

/// One configuration-reference entry suitable for generated documentation and `config explain`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigSettingDefinition {
    pub key: &'static str,
    pub meaning: &'static str,
    pub default: &'static str,
    pub allowed: &'static str,
    pub mutability: MutabilityClass,
    pub sensitive: bool,
    pub safety: &'static str,
}

/// Stable typed configuration reference.
pub const CONFIGURATION_REFERENCE: [ConfigSettingDefinition; 7] = [
    ConfigSettingDefinition {
        key: "environment",
        meaning: "closed deployment profile",
        default: "Production",
        allowed: "Development|Test|Staging|Production",
        mutability: MutabilityClass::StartupOnly,
        sensitive: false,
        safety: "profiles do not replace security controls",
    },
    ConfigSettingDefinition {
        key: "http.bind",
        meaning: "HTTP listener address",
        default: "127.0.0.1:8443",
        allowed: "non-empty socket address",
        mutability: MutabilityClass::RestartRequired,
        sensitive: false,
        safety: "production requires TLS or a trusted proxy",
    },
    ConfigSettingDefinition {
        key: "authority",
        meaning: "logical authoritative adapter",
        default: "Postgres",
        allowed: "Postgres|Neon",
        mutability: MutabilityClass::ImmutableAfterInitialization,
        sensitive: true,
        safety: "credentials are SecretRef values",
    },
    ConfigSettingDefinition {
        key: "local_store",
        meaning: "logical embedded adapter",
        default: "Sqlite",
        allowed: "Sqlite|Stoolap",
        mutability: MutabilityClass::ImmutableAfterInitialization,
        sensitive: false,
        safety: "adapter capabilities are validated",
    },
    ConfigSettingDefinition {
        key: "runtime.batch_size",
        meaning: "maximum operations per batch",
        default: "256",
        allowed: "1..=4096",
        mutability: MutabilityClass::Reloadable,
        sensitive: false,
        safety: "cannot change operation semantics",
    },
    ConfigSettingDefinition {
        key: "runtime.request_timeout_ms",
        meaning: "request timeout in milliseconds",
        default: "15000",
        allowed: "1..=604800000",
        mutability: MutabilityClass::Reloadable,
        sensitive: false,
        safety: "timeout affects progress, not atomicity",
    },
    ConfigSettingDefinition {
        key: "runtime.worker_limit",
        meaning: "bounded concurrent workers",
        default: "8",
        allowed: "1..=1024",
        mutability: MutabilityClass::RestartRequired,
        sensitive: false,
        safety: "never unbounded",
    },
];

/// Looks up exact setting metadata.
#[must_use]
pub fn explain_setting(key: &str) -> Option<&'static ConfigSettingDefinition> {
    CONFIGURATION_REFERENCE
        .iter()
        .find(|setting| setting.key == key)
}

/// One effective configuration change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigChange {
    pub key: &'static str,
    pub class: ChangeClass,
}

/// Classifies all supported changes without exposing secret values.
#[must_use]
pub fn diff_effective(current: &EffectiveConfig, candidate: &EffectiveConfig) -> Vec<ConfigChange> {
    let mut changes = Vec::new();
    if current.environment != candidate.environment {
        changes.push(ConfigChange {
            key: "environment",
            class: ChangeClass::SecuritySensitive,
        });
    }
    if current.http != candidate.http {
        changes.push(ConfigChange {
            key: "http",
            class: ChangeClass::RestartRequired,
        });
    }
    if current.authority != candidate.authority {
        changes.push(ConfigChange {
            key: "authority",
            class: ChangeClass::Forbidden,
        });
    }
    if current.local_store != candidate.local_store {
        changes.push(ConfigChange {
            key: "local_store",
            class: ChangeClass::Forbidden,
        });
    }
    changes.extend(
        diff_runtime_policy(&current.runtime, &candidate.runtime)
            .into_iter()
            .map(|change| ConfigChange {
                key: change.key,
                class: change.class,
            }),
    );
    if current.features != candidate.features {
        changes.push(ConfigChange {
            key: "features",
            class: ChangeClass::Reloadable,
        });
    }
    changes
}

/// Supported legacy v0 shape for explicit migration to the canonical schema.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyConfigV0 {
    environment: Environment,
    bind: String,
    runtime: RuntimePolicy,
}

/// Migrates one supported v0 RON document to a validated current raw model.
///
/// # Errors
///
/// Rejects malformed or unknown legacy fields and validates the migrated result.
pub fn migrate_v0(
    input: &str,
    capabilities: AdapterCapabilities,
) -> Result<ValidatedDeploymentConfig, ConfigurationError> {
    let legacy: LegacyConfigV0 =
        ron::from_str(input).map_err(|error| ConfigurationError::Ron(error.to_string()))?;
    RawDeploymentConfig {
        environment: legacy.environment,
        http: HttpDeploymentConfig {
            bind: legacy.bind,
            ..HttpDeploymentConfig::default()
        },
        runtime: legacy.runtime,
        ..RawDeploymentConfig::default()
    }
    .validate(capabilities)
}

/// Part 43 parsing, validation, migration, or capability failure.
#[derive(Debug, Error)]
pub enum ConfigurationError {
    #[error("invalid deployment RON: {0}")]
    Ron(String),
    #[error("unsupported configuration schema version {0}")]
    UnsupportedSchema(u16),
    #[error("invalid configuration value for {0}")]
    InvalidValue(&'static str),
    #[error("unknown Aequora environment override {0}")]
    UnknownEnvironmentKey(String),
    #[error("production or staging profile rejected a development-only facility")]
    ProductionSafety,
    #[error("test-only facilities were requested but not compiled")]
    TestFacilitiesNotCompiled,
    #[error("selected adapter does not declare a required certified capability")]
    UnsupportedAdapterCapability,
    #[error("configuration encoding failed: {0}")]
    Encoding(String),
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error(transparent)]
    Feature(#[from] FeatureError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> AdapterCapabilities {
        AdapterCapabilities {
            authoritative_transactions: true,
            local_atomicity: true,
            snapshot_generation_swap: true,
        }
    }

    #[test]
    fn precedence_is_fixed_and_provenance_names_the_winner() {
        let base = "(environment: Some(Development), runtime: Some((batch_size: 128, request_timeout: 9000, worker_limit: 12, log_level: Warn)))";
        let environment = "(environment: Some(Staging))";
        let mut variables = BTreeMap::new();
        variables.insert("AEQUORA_ENVIRONMENT".to_owned(), "production".to_owned());
        variables.insert("AEQUORA_BATCH_SIZE".to_owned(), "64".to_owned());
        let effective = ConfigLoader::new(RawDeploymentConfig::default())
            .environment_ron("staging.ron", environment)
            .and_then(|loader| loader.base_ron("base.ron", base))
            .and_then(|loader| loader.environment(&variables))
            .map(|loader| {
                loader.cli(ConfigPatch {
                    environment: Some(Environment::Test),
                    ..ConfigPatch::default()
                })
            })
            .and_then(|loader| loader.load(ConfigGeneration(4), capabilities()))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(effective.environment, Environment::Test);
        assert_eq!(effective.runtime.batch_size.get(), 64);
        assert_eq!(effective.runtime.worker_limit.get(), 12);
        assert_eq!(
            effective.provenance.get("environment"),
            Some(&ConfigSource::CommandLine)
        );
        assert_eq!(
            effective.provenance.get("runtime.batch_size"),
            Some(&ConfigSource::EnvironmentVariable("AEQUORA_*".to_owned()))
        );
    }

    #[test]
    fn production_rejects_development_bypass_and_unknown_fields() {
        let raw = RawDeploymentConfig {
            development: DevelopmentFacilities([DevelopmentFacility::AuthenticationBypass].into()),
            ..RawDeploymentConfig::default()
        };
        assert!(matches!(
            raw.validate(capabilities()),
            Err(ConfigurationError::ProductionSafety)
        ));
        assert!(parse_patch("(typo: Some(true))").is_err());
    }

    #[test]
    fn unsupported_adapter_capability_fails_instead_of_downgrading() {
        let result = RawDeploymentConfig::default().validate(AdapterCapabilities::default());
        assert!(matches!(
            result,
            Err(ConfigurationError::UnsupportedAdapterCapability)
        ));
    }

    #[test]
    fn sanitized_output_and_digest_never_contain_secret_marker() {
        let effective = ConfigLoader::new(RawDeploymentConfig::default())
            .load(ConfigGeneration(1), capabilities())
            .unwrap_or_else(|error| panic!("{error}"));
        let output = effective
            .sanitized_ron()
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(output.contains("<redacted>"));
        assert!(!output.contains("AEQUORA_DATABASE_URL"));
        assert!(
            !effective
                .digest
                .to_string()
                .contains("AEQUORA_DATABASE_URL")
        );
    }

    #[test]
    fn legacy_config_migrates_to_current_canonical_model() {
        let legacy = "(environment: Production, bind: \"127.0.0.1:9443\", runtime: (batch_size: 128, request_timeout: 5000, worker_limit: 4, log_level: Info))";
        let migrated = migrate_v0(legacy, capabilities()).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(migrated.0.schema_version, CURRENT_CONFIG_SCHEMA_VERSION);
        assert_eq!(migrated.0.http.bind, "127.0.0.1:9443");
    }
}
