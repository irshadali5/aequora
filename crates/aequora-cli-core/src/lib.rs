//! Typed, transport-neutral contracts for Aequora command-line clients.
//!
//! This crate deliberately does not own synchronization, storage, migration execution, or
//! control-plane semantics. It validates CLI intent and renders stable envelopes around results
//! returned by the public SDK and control-plane clients.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, str::FromStr};
use thiserror::Error;

/// Version of the stable machine-readable CLI envelope.
pub const MACHINE_SCHEMA_VERSION: u16 = 1;

/// Stable process exit categories. Values are part of the automation contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum CliExitCode {
    Success = 0,
    UsageOrConfiguration = 2,
    AuthenticationOrAuthorization = 3,
    Compatibility = 4,
    ValidationOrConformance = 5,
    TemporaryUnavailable = 6,
    OperationFailed = 7,
    ManualIntervention = 8,
}

/// Stable error categories used in machine output.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidUsage,
    InvalidConfiguration,
    Unauthorized,
    Incompatible,
    ValidationFailed,
    DependencyUnavailable,
    OperationFailed,
    ManualInterventionRequired,
}

impl ErrorCode {
    /// Maps an error to its stable process exit category.
    #[must_use]
    pub const fn exit_code(self) -> CliExitCode {
        match self {
            Self::InvalidUsage | Self::InvalidConfiguration => CliExitCode::UsageOrConfiguration,
            Self::Unauthorized => CliExitCode::AuthenticationOrAuthorization,
            Self::Incompatible => CliExitCode::Compatibility,
            Self::ValidationFailed => CliExitCode::ValidationOrConformance,
            Self::DependencyUnavailable => CliExitCode::TemporaryUnavailable,
            Self::OperationFailed => CliExitCode::OperationFailed,
            Self::ManualInterventionRequired => CliExitCode::ManualIntervention,
        }
    }
}

/// Whether retrying the same semantic request is appropriate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RetryClass {
    Never,
    RetrySameIdentity,
    AfterDependencyRecovery,
    InspectBeforeRetry,
}

/// Sanitized structured details. Sensitive keys are always rendered as redacted.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SafeDetails(BTreeMap<String, String>);

impl SafeDetails {
    /// Adds a value after applying the stable key-based redaction policy.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let value = if is_sensitive_key(&key) {
            "<redacted>".to_owned()
        } else {
            value.into()
        };
        self.0.insert(key, value);
        self
    }

    /// Returns the sanitized key/value details.
    #[must_use]
    pub const fn values(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "authorization",
        "credential",
        "database_url",
        "password",
        "private_key",
        "secret",
        "token",
    ]
    .iter()
    .any(|candidate| normalized.contains(candidate))
}

/// Stable machine error envelope. Callers must supply an already-safe public message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CliErrorEnvelope {
    pub schema_version: u16,
    pub code: ErrorCode,
    pub retry: RetryClass,
    pub message: String,
    pub details: Option<SafeDetails>,
}

impl CliErrorEnvelope {
    #[must_use]
    pub fn new(code: ErrorCode, retry: RetryClass, message: impl Into<String>) -> Self {
        Self {
            schema_version: MACHINE_SCHEMA_VERSION,
            code,
            retry,
            message: message.into(),
            details: None,
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: SafeDetails) -> Self {
        self.details = Some(details);
        self
    }

    #[must_use]
    pub const fn exit_code(&self) -> CliExitCode {
        self.code.exit_code()
    }
}

/// Output representation selected independently from command execution.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
    Ron,
}

impl FromStr for OutputFormat {
    type Err = ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "human" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            "ron" => Ok(Self::Ron),
            _ => Err(ParseError::InvalidOptionValue {
                option: "--output",
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum LogFormat {
    #[default]
    Human,
    Json,
}

/// Global presentation and tracing options, kept out of command semantics.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct GlobalOptions {
    pub output: OutputFormat,
    pub color: ColorChoice,
    pub log_format: LogFormat,
    pub log_level: Option<String>,
    pub trace_id: Option<String>,
}

/// Stable top-level nouns. Adding a command does not change existing spellings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum CommandName {
    Init,
    Doctor,
    Config,
    Client,
    Sync,
    Operation,
    Conflict,
    Scope,
    Registry,
    Schema,
    Migrate,
    Adapter,
    Conform,
    Snapshot,
    Integrity,
    Repair,
    Job,
    Consumer,
    Authority,
    Diagnostics,
    Incident,
    Admin,
    Dev,
    Version,
    Inspect,
    Verify,
    Queue,
    Import,
    Bootstrap,
    Region,
    Compat,
    Compatibility,
    Security,
    Feed,
    Legacy,
    Bench,
    Load,
    Capacity,
    Deps,
    SupplyChain,
}

impl CommandName {
    pub const ALL: [Self; 40] = [
        Self::Init,
        Self::Doctor,
        Self::Config,
        Self::Client,
        Self::Sync,
        Self::Operation,
        Self::Conflict,
        Self::Scope,
        Self::Registry,
        Self::Schema,
        Self::Migrate,
        Self::Adapter,
        Self::Conform,
        Self::Snapshot,
        Self::Integrity,
        Self::Repair,
        Self::Job,
        Self::Consumer,
        Self::Authority,
        Self::Diagnostics,
        Self::Incident,
        Self::Admin,
        Self::Dev,
        Self::Version,
        Self::Inspect,
        Self::Verify,
        Self::Queue,
        Self::Import,
        Self::Bootstrap,
        Self::Region,
        Self::Compat,
        Self::Compatibility,
        Self::Security,
        Self::Feed,
        Self::Legacy,
        Self::Bench,
        Self::Load,
        Self::Capacity,
        Self::Deps,
        Self::SupplyChain,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Doctor => "doctor",
            Self::Config => "config",
            Self::Client => "client",
            Self::Sync => "sync",
            Self::Operation => "operation",
            Self::Conflict => "conflict",
            Self::Scope => "scope",
            Self::Registry => "registry",
            Self::Schema => "schema",
            Self::Migrate => "migrate",
            Self::Adapter => "adapter",
            Self::Conform => "conform",
            Self::Snapshot => "snapshot",
            Self::Integrity => "integrity",
            Self::Repair => "repair",
            Self::Job => "job",
            Self::Consumer => "consumer",
            Self::Authority => "authority",
            Self::Diagnostics => "diagnostics",
            Self::Incident => "incident",
            Self::Admin => "admin",
            Self::Dev => "dev",
            Self::Version => "version",
            Self::Inspect => "inspect",
            Self::Verify => "verify",
            Self::Queue => "queue",
            Self::Import => "import",
            Self::Bootstrap => "bootstrap",
            Self::Region => "region",
            Self::Compat => "compat",
            Self::Compatibility => "compatibility",
            Self::Security => "security",
            Self::Feed => "feed",
            Self::Legacy => "legacy",
            Self::Bench => "bench",
            Self::Load => "load",
            Self::Capacity => "capacity",
            Self::Deps => "deps",
            Self::SupplyChain => "supply-chain",
        }
    }
}

impl fmt::Display for CommandName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CommandName {
    type Err = ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|name| name.as_str() == value)
            .ok_or_else(|| ParseError::UnknownCommand(value.to_owned()))
    }
}

/// Typed command after global parsing. Subcommand arguments remain owned and ordered for the
/// domain-specific command executor; they are never flattened into an untyped string map.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Command {
    Help,
    Invoke {
        name: CommandName,
        arguments: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CliRequest {
    pub global: GlobalOptions,
    pub command: Command,
}

impl CliRequest {
    /// Parses global options and the stable top-level command hierarchy.
    ///
    /// # Errors
    ///
    /// Returns a typed parse error for unknown commands, options, or missing option values.
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, ParseError> {
        let mut values = arguments.into_iter().peekable();
        let mut global = GlobalOptions::default();
        loop {
            let Some(option) = values.peek().map(String::as_str) else {
                return Ok(Self {
                    global,
                    command: Command::Help,
                });
            };
            if !option.starts_with('-') || matches!(option, "-h" | "--help") {
                break;
            }
            let option = values.next().ok_or(ParseError::MissingCommand)?;
            let value = values
                .next()
                .ok_or_else(|| ParseError::MissingOptionValue(option.clone()))?;
            match option.as_str() {
                "--output" => global.output = value.parse()?,
                "--color" => {
                    global.color = match value.as_str() {
                        "auto" => ColorChoice::Auto,
                        "always" => ColorChoice::Always,
                        "never" => ColorChoice::Never,
                        _ => {
                            return Err(ParseError::InvalidOptionValue {
                                option: "--color",
                                value,
                            });
                        }
                    };
                }
                "--log-format" => {
                    global.log_format = match value.as_str() {
                        "human" => LogFormat::Human,
                        "json" => LogFormat::Json,
                        _ => {
                            return Err(ParseError::InvalidOptionValue {
                                option: "--log-format",
                                value,
                            });
                        }
                    };
                }
                "--log-level" => global.log_level = Some(value),
                "--trace-id" => global.trace_id = Some(value),
                _ => return Err(ParseError::UnknownOption(option)),
            }
        }

        let Some(name) = values.next() else {
            return Ok(Self {
                global,
                command: Command::Help,
            });
        };
        if matches!(name.as_str(), "help" | "-h" | "--help") {
            return Ok(Self {
                global,
                command: Command::Help,
            });
        }
        Ok(Self {
            global,
            command: Command::Invoke {
                name: name.parse()?,
                arguments: values.collect(),
            },
        })
    }

    #[must_use]
    pub fn execution_arguments(&self) -> Vec<String> {
        match &self.command {
            Command::Help => vec!["help".to_owned()],
            Command::Invoke { name, arguments } => {
                let mut result = Vec::with_capacity(arguments.len() + 1);
                result.push(name.as_str().to_owned());
                result.extend(arguments.iter().cloned());
                result
            }
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ParseError {
    #[error("missing command")]
    MissingCommand,
    #[error("unknown command {0:?}")]
    UnknownCommand(String),
    #[error("unknown global option {0:?}")]
    UnknownOption(String),
    #[error("missing value for global option {0:?}")]
    MissingOptionValue(String),
    #[error("invalid value {value:?} for {option}")]
    InvalidOptionValue { option: &'static str, value: String },
}

/// Operational risk of one semantic command.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SafetyClass {
    ReadOnly,
    LowRiskMutation,
    OperationalMutation,
    Destructive,
    AuthorityCritical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Permission {
    Read,
    Mutate,
    Admin,
    Authority,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommandPolicy {
    pub safety: SafetyClass,
    pub required_permissions: Vec<Permission>,
    pub supports_dry_run: bool,
    pub requires_plan: bool,
    pub requires_reason: bool,
}

impl CommandPolicy {
    #[must_use]
    pub fn for_command(name: CommandName) -> Self {
        match name {
            CommandName::Authority | CommandName::Repair => Self {
                safety: SafetyClass::AuthorityCritical,
                required_permissions: vec![Permission::Authority],
                supports_dry_run: true,
                requires_plan: true,
                requires_reason: true,
            },
            CommandName::Admin | CommandName::Consumer | CommandName::Migrate => Self {
                safety: SafetyClass::Destructive,
                required_permissions: vec![Permission::Admin],
                supports_dry_run: true,
                requires_plan: true,
                requires_reason: true,
            },
            CommandName::Sync
            | CommandName::Client
            | CommandName::Operation
            | CommandName::Conflict
            | CommandName::Scope
            | CommandName::Job
            | CommandName::Snapshot => Self {
                safety: SafetyClass::OperationalMutation,
                required_permissions: vec![Permission::Mutate],
                supports_dry_run: false,
                requires_plan: false,
                requires_reason: false,
            },
            CommandName::Init | CommandName::Dev | CommandName::Import | CommandName::Queue => {
                Self {
                    safety: SafetyClass::LowRiskMutation,
                    required_permissions: vec![Permission::Mutate],
                    supports_dry_run: true,
                    requires_plan: false,
                    requires_reason: false,
                }
            }
            _ => Self {
                safety: SafetyClass::ReadOnly,
                required_permissions: vec![Permission::Read],
                supports_dry_run: false,
                requires_plan: false,
                requires_reason: false,
            },
        }
    }
}

macro_rules! validated_identity {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        pub struct $name(String);

        impl $name {
            /// Parses a bounded, printable stable identity.
            ///
            /// # Errors
            ///
            /// Returns [`IdentityError`] when the value is empty, too long, or not portable ASCII.
            pub fn parse(value: impl Into<String>) -> Result<Self, IdentityError> {
                let value = value.into();
                if value.is_empty() || value.len() > 128 {
                    return Err(IdentityError::InvalidLength);
                }
                if !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return Err(IdentityError::InvalidCharacter);
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

validated_identity!(PlanId);
validated_identity!(AdminOperationId);

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum IdentityError {
    #[error("identity length must be between 1 and 128 bytes")]
    InvalidLength,
    #[error("identity must contain only portable ASCII letters, digits, '-' or '_'")]
    InvalidCharacter,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplyIntent {
    pub plan_id: PlanId,
    pub operation_id: AdminOperationId,
    pub reason: String,
    pub authorized: bool,
    pub step_up_authorized: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PolicyError {
    #[error("this action requires a reviewed plan")]
    PlanRequired,
    #[error("this action requires a non-empty audit reason")]
    ReasonRequired,
    #[error("server authorization is required")]
    AuthorizationRequired,
    #[error("authority-critical action requires step-up authorization")]
    StepUpRequired,
}

/// Validates client-side intent metadata. The service remains the authoritative policy boundary.
///
/// # Errors
///
/// Returns the first missing safety condition declared by `policy`.
pub fn validate_apply(
    policy: &CommandPolicy,
    intent: Option<&ApplyIntent>,
) -> Result<(), PolicyError> {
    if policy.requires_plan && intent.is_none() {
        return Err(PolicyError::PlanRequired);
    }
    let Some(intent) = intent else {
        return Ok(());
    };
    if policy.requires_reason && intent.reason.trim().is_empty() {
        return Err(PolicyError::ReasonRequired);
    }
    if !intent.authorized {
        return Err(PolicyError::AuthorizationRequired);
    }
    if policy.safety == SafetyClass::AuthorityCritical && !intent.step_up_authorized {
        return Err(PolicyError::StepUpRequired);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WaitOutcome {
    Completed,
    StoppedWaiting,
    ResponseLost,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SubmissionState {
    NotSubmitted,
    DurablySubmitted {
        operation_id: AdminOperationId,
        wait: WaitOutcome,
    },
}

impl SubmissionState {
    /// Returns true only when no durable submission is known to have occurred.
    #[must_use]
    pub const fn definitely_not_executed(&self) -> bool {
        matches!(self, Self::NotSubmitted)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationIdentity {
    pub migration_id: String,
    pub checksum: String,
    pub source_version: u64,
    pub target_version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationPreconditions {
    pub observed_source_version: u64,
    pub observed_checksum: String,
    pub store_identity_matches: bool,
    pub capabilities_satisfied: bool,
    pub exclusive_fence_acquired: bool,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MigrationVerificationError {
    #[error("migration checksum does not match the reviewed plan")]
    ChecksumMismatch,
    #[error("store is not at the reviewed source version")]
    SourceVersionMismatch,
    #[error("store or authority identity does not match the reviewed target")]
    StoreIdentityMismatch,
    #[error("required adapter capabilities are unavailable")]
    MissingCapability,
    #[error("exclusive fenced maintenance ownership is required")]
    FenceRequired,
}

/// Verifies immutable migration identity and runtime preconditions before service execution.
///
/// # Errors
///
/// Fails closed on any checksum, version, store, capability, or fencing mismatch.
pub fn verify_migration(
    migration: &MigrationIdentity,
    observed: &MigrationPreconditions,
) -> Result<(), MigrationVerificationError> {
    if migration.checksum != observed.observed_checksum {
        return Err(MigrationVerificationError::ChecksumMismatch);
    }
    if migration.source_version != observed.observed_source_version {
        return Err(MigrationVerificationError::SourceVersionMismatch);
    }
    if !observed.store_identity_matches {
        return Err(MigrationVerificationError::StoreIdentityMismatch);
    }
    if !observed.capabilities_satisfied {
        return Err(MigrationVerificationError::MissingCapability);
    }
    if !observed.exclusive_fence_acquired {
        return Err(MigrationVerificationError::FenceRequired);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DoctorCheck {
    pub id: String,
    pub status: CheckStatus,
    pub summary: String,
    pub remediation_available: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    pub writes: u64,
}

impl DoctorReport {
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.writes == 0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MachineEnvelope<T> {
    pub schema_version: u16,
    pub command: String,
    pub result: T,
}

impl<T> MachineEnvelope<T> {
    #[must_use]
    pub fn new(command: impl Into<String>, result: T) -> Self {
        Self {
            schema_version: MACHINE_SCHEMA_VERSION,
            command: command.into(),
            result,
        }
    }
}

/// Renders a serializable result using the selected stable machine format.
///
/// # Errors
///
/// Returns a serialization error if `value` cannot be represented.
pub fn render_machine<T: Serialize>(
    format: OutputFormat,
    value: &T,
) -> Result<String, RenderError> {
    match format {
        OutputFormat::Human => Err(RenderError::HumanRequiresRenderer),
        OutputFormat::Json => serde_json::to_string_pretty(value)
            .map_err(|error| RenderError::Serialize(error.to_string())),
        OutputFormat::Ron => ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())
            .map_err(|error| RenderError::Serialize(error.to_string())),
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RenderError {
    #[error("human output requires a command-specific renderer")]
    HumanRequiresRenderer,
    #[error("machine output serialization failed: {0}")]
    Serialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_separates_global_output_from_typed_command() {
        let request =
            CliRequest::parse(["--output", "json", "sync", "now", "--wait"].map(str::to_owned));
        let request = request.unwrap_or_else(|error| panic!("parse failed: {error}"));
        assert_eq!(request.global.output, OutputFormat::Json);
        assert_eq!(request.execution_arguments(), ["sync", "now", "--wait"]);
    }

    #[test]
    fn machine_output_has_a_stable_schema_version() {
        let output = render_machine(
            OutputFormat::Json,
            &MachineEnvelope::new("doctor", DoctorReport::default()),
        );
        let output = output.unwrap_or_else(|error| panic!("render failed: {error}"));
        assert!(output.contains("\"schema_version\": 1"));
    }

    #[test]
    fn sensitive_detail_keys_are_redacted() {
        let details = SafeDetails::default()
            .with("DATABASE_URL", "postgres://admin:secret@example/db")
            .with("source", "environment");
        assert_eq!(
            details.values().get("DATABASE_URL").map(String::as_str),
            Some("<redacted>")
        );
        assert_eq!(
            details.values().get("source").map(String::as_str),
            Some("environment")
        );
    }

    #[test]
    fn authority_apply_requires_plan_reason_authorization_and_step_up() {
        let policy = CommandPolicy::for_command(CommandName::Authority);
        assert_eq!(
            validate_apply(&policy, None),
            Err(PolicyError::PlanRequired)
        );
        let intent = ApplyIntent {
            plan_id: PlanId::parse("plan-1").unwrap_or_else(|error| panic!("id failed: {error}")),
            operation_id: AdminOperationId::parse("op-1")
                .unwrap_or_else(|error| panic!("id failed: {error}")),
            reason: "approved failover".to_owned(),
            authorized: true,
            step_up_authorized: true,
        };
        assert_eq!(validate_apply(&policy, Some(&intent)), Ok(()));
    }

    #[test]
    fn timeout_after_submission_never_means_not_executed() {
        let state = SubmissionState::DurablySubmitted {
            operation_id: AdminOperationId::parse("op-2")
                .unwrap_or_else(|error| panic!("id failed: {error}")),
            wait: WaitOutcome::StoppedWaiting,
        };
        assert!(!state.definitely_not_executed());
    }

    #[test]
    fn migration_verification_fails_closed() {
        let migration = MigrationIdentity {
            migration_id: "m1".to_owned(),
            checksum: "abc".to_owned(),
            source_version: 1,
            target_version: 2,
        };
        let observed = MigrationPreconditions {
            observed_source_version: 1,
            observed_checksum: "changed".to_owned(),
            store_identity_matches: true,
            capabilities_satisfied: true,
            exclusive_fence_acquired: true,
        };
        assert_eq!(
            verify_migration(&migration, &observed),
            Err(MigrationVerificationError::ChecksumMismatch)
        );
    }
}
