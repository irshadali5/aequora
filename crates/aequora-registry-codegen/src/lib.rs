//! Deterministic loader, validator, compatibility checker, and generator.

use aequora_registry_types::{
    AllocationClass, ChangeClassification, LockEntry, RegistryDomain, RegistryEntry, RegistryError,
    RegistryFragment, RegistryLock, RegistryManifest, RegistrySet, SupportStatus,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};
use thiserror::Error;

const MAX_REGISTRY_FILE_BYTES: usize = 1_024 * 1_024;
const MAX_REGISTRY_PATHS: usize = 8_192;
const MAX_REGISTRY_DEPTH: usize = 16;
const MAX_REGISTRY_ENTRIES: usize = 65_536;
const MAX_ENTRIES_PER_FRAGMENT: usize = 4_096;
const MAX_ENTRY_TEXT_BYTES: usize = 4_096;
const MAX_ENTRY_NAME_BYTES: usize = 128;
const MAX_ENTRY_COLLECTION_ITEMS: usize = 256;

const REQUIRED_OPERATION_ATTRIBUTES: &[&str] = &[
    "entity",
    "profile",
    "ordering",
    "offline",
    "compaction",
    "rebase",
    "authorization",
    "handler",
];
const REQUIRED_FIELD_ATTRIBUTES: &[&str] = &[
    "entity",
    "data_class",
    "audit_policy",
    "conflict_policy",
    "retention",
    "encryption",
];

#[derive(Debug, Error)]
pub enum CodegenError {
    #[error("registry I/O failed at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("registry RON failed at {path}: {message}")]
    Ron { path: PathBuf, message: String },
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("generated artifact drift at {0}")]
    Drift(PathBuf),
    #[error("registry input exceeds its safety limit at {path}: {limit}")]
    Limit { path: PathBuf, limit: &'static str },
    #[error("registry input is not a regular UTF-8 file at {0}")]
    InvalidFile(PathBuf),
    #[error("registry traversal rejected a symbolic link at {0}")]
    Symlink(PathBuf),
}

fn read(path: &Path) -> Result<String, CodegenError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| CodegenError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(CodegenError::Symlink(path.to_path_buf()));
    }
    if !metadata.is_file() {
        return Err(CodegenError::InvalidFile(path.to_path_buf()));
    }
    let mut bytes = Vec::with_capacity(MAX_REGISTRY_FILE_BYTES.min(64 * 1_024));
    File::open(path)
        .map_err(|source| CodegenError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .take(
            u64::try_from(MAX_REGISTRY_FILE_BYTES)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .map_err(|source| CodegenError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_REGISTRY_FILE_BYTES {
        return Err(CodegenError::Limit {
            path: path.to_path_buf(),
            limit: "file bytes",
        });
    }
    String::from_utf8(bytes).map_err(|_| CodegenError::InvalidFile(path.to_path_buf()))
}

/// Loads and merges all canonical fragments under `registry/core`, `registry/app`, and
/// `registry/extensions` in lexical order.
///
/// # Errors
///
/// Returns an I/O, parse, or validation error.
pub fn load_registry(root: &Path) -> Result<RegistrySet, CodegenError> {
    let registry_root = root.join("registry");
    let manifest_path = registry_root.join("manifest.ron");
    let manifest = ron::from_str::<RegistryManifest>(&read(&manifest_path)?).map_err(|error| {
        CodegenError::Ron {
            path: manifest_path,
            message: error.to_string(),
        }
    })?;
    let mut paths = Vec::new();
    let mut visited_paths = 0;
    for directory in ["core", "app", "extensions"] {
        collect_ron(
            &registry_root.join(directory),
            &mut paths,
            0,
            &mut visited_paths,
        )?;
    }
    paths.sort();
    let mut entries = Vec::new();
    for path in paths {
        let fragment = ron::from_str::<RegistryFragment>(&read(&path)?).map_err(|error| {
            CodegenError::Ron {
                path: path.clone(),
                message: error.to_string(),
            }
        })?;
        if fragment.entries.len() > MAX_ENTRIES_PER_FRAGMENT
            || entries.len().saturating_add(fragment.entries.len()) > MAX_REGISTRY_ENTRIES
        {
            return Err(CodegenError::Limit {
                path,
                limit: "registry entries",
            });
        }
        entries.extend(fragment.entries);
    }
    entries.sort_by_key(|entry| (entry.domain, entry.id));
    let set = RegistrySet { manifest, entries };
    validate(&set)?;
    Ok(set)
}

fn collect_ron(
    directory: &Path,
    output: &mut Vec<PathBuf>,
    depth: usize,
    visited_paths: &mut usize,
) -> Result<(), CodegenError> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(CodegenError::Io {
                path: directory.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(CodegenError::Symlink(directory.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(CodegenError::InvalidFile(directory.to_path_buf()));
    }
    if depth > MAX_REGISTRY_DEPTH {
        return Err(CodegenError::Limit {
            path: directory.to_path_buf(),
            limit: "directory depth",
        });
    }
    let entries = fs::read_dir(directory).map_err(|source| CodegenError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        *visited_paths = visited_paths.saturating_add(1);
        if *visited_paths > MAX_REGISTRY_PATHS {
            return Err(CodegenError::Limit {
                path: directory.to_path_buf(),
                limit: "traversed paths",
            });
        }
        let entry = entry.map_err(|source| CodegenError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| CodegenError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(CodegenError::Symlink(path));
        }
        if file_type.is_dir() {
            collect_ron(&path, output, depth.saturating_add(1), visited_paths)?;
        } else if file_type.is_file()
            && path.extension().is_some_and(|extension| extension == "ron")
        {
            output.push(path);
        }
    }
    Ok(())
}

/// Validates collisions, ranges, lifecycle metadata, dependency references, schema metadata, and
/// fail-closed security policy.
///
/// # Errors
///
/// Returns the first deterministic registry error.
pub fn validate(set: &RegistrySet) -> Result<(), RegistryError> {
    if set.manifest.generation == 0 {
        return Err(RegistryError::ZeroGeneration);
    }
    if !valid_text(&set.manifest.release, MAX_ENTRY_NAME_BYTES)
        || !valid_text(&set.manifest.namespace, MAX_ENTRY_NAME_BYTES)
    {
        return Err(RegistryError::InvalidManifest);
    }
    if set.entries.len() > MAX_REGISTRY_ENTRIES {
        return Err(RegistryError::RegistryLimit);
    }
    let mut keys = BTreeSet::new();
    let mut names = BTreeSet::new();
    for entry in &set.entries {
        if !keys.insert((entry.domain, entry.id)) {
            return Err(RegistryError::DuplicateId {
                domain: entry.domain,
                id: entry.id,
            });
        }
        if !names.insert((entry.domain, entry.name.clone())) {
            return Err(RegistryError::DuplicateName {
                domain: entry.domain,
                name: entry.name.clone(),
            });
        }
        validate_entry(entry)?;
    }
    for entry in &set.entries {
        for reference in entry.references.iter().chain(entry.replacement.iter()) {
            if !keys.contains(&(reference.domain, reference.id)) {
                return Err(RegistryError::UnresolvedReference {
                    domain: reference.domain,
                    id: reference.id,
                });
            }
        }
    }
    Ok(())
}

fn invalid(entry: &RegistryEntry, reason: impl Into<String>) -> RegistryError {
    RegistryError::InvalidEntry {
        domain: entry.domain,
        id: entry.id,
        reason: reason.into(),
    }
}

#[allow(clippy::too_many_lines)]
fn validate_entry(entry: &RegistryEntry) -> Result<(), RegistryError> {
    if entry.id == 0 || entry.name.len() > MAX_ENTRY_NAME_BYTES || !is_canonical_name(&entry.name) {
        return Err(invalid(
            entry,
            "ID must be non-zero and name must be UpperCamelCase",
        ));
    }
    if !valid_text(&entry.owner.team, MAX_ENTRY_NAME_BYTES)
        || !valid_text(&entry.owner.crate_name, MAX_ENTRY_NAME_BYTES)
        || !valid_text(&entry.owner.module, MAX_ENTRY_NAME_BYTES)
        || !valid_text(&entry.description, MAX_ENTRY_TEXT_BYTES)
        || !valid_text(&entry.introduced_in, MAX_ENTRY_NAME_BYTES)
        || entry.supported_schema_versions.len() > MAX_ENTRY_COLLECTION_ITEMS
        || entry.attributes.len() > MAX_ENTRY_COLLECTION_ITEMS
        || entry.references.len() > MAX_ENTRY_COLLECTION_ITEMS
        || entry.attributes.iter().any(|(key, value)| {
            !valid_text(key, MAX_ENTRY_NAME_BYTES) || !valid_text(value, MAX_ENTRY_TEXT_BYTES)
        })
        || entry
            .change_proposal_ref
            .as_deref()
            .is_some_and(|value| !valid_text(value, MAX_ENTRY_TEXT_BYTES))
        || entry
            .migration_ref
            .as_deref()
            .is_some_and(|value| !valid_text(value, MAX_ENTRY_TEXT_BYTES))
    {
        return Err(invalid(
            entry,
            "owner, description, and introduced release are required",
        ));
    }
    match (entry.allocation_class(), entry.status) {
        (AllocationClass::Experimental, SupportStatus::Experimental) => {}
        (AllocationClass::Experimental, _) => {
            return Err(invalid(
                entry,
                "experimental-range IDs must remain Experimental",
            ));
        }
        (_, SupportStatus::Experimental) => {
            return Err(invalid(
                entry,
                "Experimental entries must use the experimental range",
            ));
        }
        _ => {}
    }
    if entry.security_sensitive
        && entry
            .change_proposal_ref
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Err(RegistryError::MissingSecurityReview {
            domain: entry.domain,
            id: entry.id,
        });
    }
    if matches!(
        entry.status,
        SupportStatus::Deprecated | SupportStatus::RetryOnly
    ) && entry.replacement.is_none()
        && entry.migration_ref.as_deref().is_none_or(str::is_empty)
    {
        return Err(invalid(
            entry,
            "deprecated entries require replacement or migration metadata",
        ));
    }
    if entry.supported_schema_versions.contains(&0)
        || entry
            .supported_schema_versions
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != entry.supported_schema_versions.len()
        || entry.schema_version.is_none() && !entry.supported_schema_versions.is_empty()
    {
        return Err(invalid(entry, "schema version history is invalid"));
    }
    if let Some(version) = entry.schema_version {
        if version == 0 || !entry.supported_schema_versions.contains(&version) {
            return Err(invalid(
                entry,
                "current schema must be non-zero and included in supported versions",
            ));
        }
    }
    let required = match entry.domain {
        RegistryDomain::Operation => REQUIRED_OPERATION_ATTRIBUTES,
        RegistryDomain::Field => REQUIRED_FIELD_ATTRIBUTES,
        RegistryDomain::Entity => &["aggregate_root", "profile"][..],
        RegistryDomain::Event => &["visibility", "audit_relation"][..],
        RegistryDomain::Capability => &["requirement", "fallback"][..],
        RegistryDomain::Error => &["retry_class", "http_mapping", "user_safe_category"][..],
        RegistryDomain::Job => &["retry_policy", "epoch_recovery", "concurrency"][..],
        RegistryDomain::Consumer => &["ordering", "retention", "visibility"][..],
        RegistryDomain::Migration => &["kind", "checksum"][..],
        RegistryDomain::Protocol => &["compatibility"][..],
        RegistryDomain::ConformanceProfile => &["domains", "minimum_tier"][..],
        RegistryDomain::ConformanceTest => &["domain", "minimum_tier", "invariants"][..],
        RegistryDomain::CertificationTier => &["rank", "required_result"][..],
        _ => &[],
    };
    if let Some(missing) = required
        .iter()
        .find(|key| !entry.attributes.contains_key(**key))
    {
        return Err(invalid(
            entry,
            format!("missing required attribute `{missing}`"),
        ));
    }
    if entry.domain == RegistryDomain::Capability
        && entry.attribute("requirement") == Some("RequiredForSafety")
        && entry.attribute("fallback") != Some("Reject")
    {
        return Err(invalid(
            entry,
            "RequiredForSafety capabilities must reject rather than fall back",
        ));
    }
    Ok(())
}

fn is_canonical_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= maximum
        && value.chars().all(|character| !character.is_control())
}

/// Computes the stable digest of semantic fields; prose and ownership changes are intentionally
/// excluded.
#[must_use]
pub fn semantic_digest(entry: &RegistryEntry) -> String {
    let mut bytes = format!(
        "v1|{}|{}|{}|{:?}|{:?}|{:?}|",
        entry.domain,
        entry.id,
        entry.name,
        entry.schema_version,
        entry.supported_schema_versions,
        entry.security_sensitive
    );
    for (key, value) in &entry.attributes {
        let _ = write!(
            bytes,
            "{}={}|",
            escape_semantic(key),
            escape_semantic(value)
        );
    }
    for reference in &entry.references {
        let _ = write!(bytes, "{}:{}|", reference.domain, reference.id);
    }
    if let Some(replacement) = entry.replacement {
        let _ = write!(
            bytes,
            "replacement={}:{}|",
            replacement.domain, replacement.id
        );
    }
    blake3::hash(bytes.as_bytes()).to_hex().to_string()
}

fn escape_semantic(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace('=', "%3D")
}

#[must_use]
pub fn make_lock(set: &RegistrySet) -> RegistryLock {
    RegistryLock {
        generation: set.manifest.generation,
        entries: set
            .entries
            .iter()
            .map(|entry| LockEntry {
                domain: entry.domain,
                id: entry.id,
                name: entry.name.clone(),
                semantic_digest: semantic_digest(entry),
                status: entry.status,
            })
            .collect(),
    }
}

/// Verifies that all published IDs retain their original meaning and that generation/schema
/// versions do not regress.
///
/// # Errors
///
/// Returns an error on deletion, reuse, digest drift, or regression.
pub fn verify_lock(set: &RegistrySet, lock: &RegistryLock) -> Result<(), RegistryError> {
    if set.manifest.generation < lock.generation {
        return Err(RegistryError::ZeroGeneration);
    }
    let current = set
        .entries
        .iter()
        .map(|entry| ((entry.domain, entry.id), entry))
        .collect::<BTreeMap<_, _>>();
    for published in &lock.entries {
        let Some(entry) = current.get(&(published.domain, published.id)) else {
            return Err(RegistryError::PublishedIdChanged {
                domain: published.domain,
                id: published.id,
            });
        };
        if entry.name != published.name || semantic_digest(entry) != published.semantic_digest {
            return Err(RegistryError::PublishedIdChanged {
                domain: published.domain,
                id: published.id,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryChange {
    pub classification: ChangeClassification,
    pub domain: RegistryDomain,
    pub id: u32,
    pub summary: String,
}

/// Produces a stable, payload-free compatibility report.
#[must_use]
pub fn diff(old: &RegistrySet, new: &RegistrySet) -> Vec<RegistryChange> {
    let old_by_key = old
        .entries
        .iter()
        .map(|entry| ((entry.domain, entry.id), entry))
        .collect::<BTreeMap<_, _>>();
    let new_by_key = new
        .entries
        .iter()
        .map(|entry| ((entry.domain, entry.id), entry))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for (key, old_entry) in &old_by_key {
        let Some(new_entry) = new_by_key.get(key) else {
            changes.push(RegistryChange {
                classification: ChangeClassification::BreakingWithMigration,
                domain: key.0,
                id: key.1,
                summary: "published ID deleted".into(),
            });
            continue;
        };
        if new_entry.schema_version < old_entry.schema_version {
            changes.push(RegistryChange {
                classification: ChangeClassification::BreakingWithMigration,
                domain: key.0,
                id: key.1,
                summary: "schema version regressed".into(),
            });
        } else if semantic_digest(old_entry) != semantic_digest(new_entry) {
            changes.push(RegistryChange {
                classification: if new_entry.security_sensitive {
                    ChangeClassification::SecurityRequired
                } else {
                    ChangeClassification::BreakingWithMigration
                },
                domain: key.0,
                id: key.1,
                summary: "published semantics changed".into(),
            });
        } else if old_entry.status != new_entry.status {
            changes.push(RegistryChange {
                classification: ChangeClassification::Deprecated,
                domain: key.0,
                id: key.1,
                summary: format!("status {:?} -> {:?}", old_entry.status, new_entry.status),
            });
        }
    }
    for (key, entry) in new_by_key {
        if !old_by_key.contains_key(&key) {
            changes.push(RegistryChange {
                classification: ChangeClassification::Additive,
                domain: key.0,
                id: key.1,
                summary: format!("added {}", entry.name),
            });
        }
    }
    changes.sort_by_key(|change| (change.domain, change.id));
    changes
}

/// Generates immutable Rust lookup tables.
#[must_use]
pub fn generated_rust(set: &RegistrySet) -> String {
    let mut output = String::from("// @generated by aequora-registry-codegen; do not edit.\n");
    let _ = writeln!(
        output,
        "pub const REGISTRY_GENERATION: u64 = {};",
        set.manifest.generation
    );
    let digest = registry_digest(set);
    let _ = writeln!(output, "pub const REGISTRY_DIGEST: &str = \"{digest}\";");
    output.push_str("pub static ENTRIES: &[GeneratedEntry] = &[\n");
    for entry in &set.entries {
        let _ = writeln!(
            output,
            "    GeneratedEntry {{ domain: RegistryDomain::{:?}, id: {}, name: {:?}, status: SupportStatus::{:?}, schema_version: {:?}, owner: {:?}, description: {:?} }},",
            entry.domain,
            entry.id,
            entry.name,
            entry.status,
            entry.schema_version,
            entry.owner.team,
            entry.description
        );
    }
    output.push_str("];\n");
    output.push_str("pub mod ids {\n");
    for domain in [
        RegistryDomain::Entity,
        RegistryDomain::Operation,
        RegistryDomain::Event,
        RegistryDomain::Field,
        RegistryDomain::Capability,
        RegistryDomain::ConsistencyProfile,
        RegistryDomain::Error,
        RegistryDomain::Job,
        RegistryDomain::Consumer,
        RegistryDomain::AuditAction,
        RegistryDomain::Migration,
        RegistryDomain::Protocol,
        RegistryDomain::Message,
        RegistryDomain::Permission,
        RegistryDomain::AdminAction,
        RegistryDomain::Reason,
        RegistryDomain::DecisionRule,
        RegistryDomain::ArtifactFormat,
        RegistryDomain::ConformanceProfile,
        RegistryDomain::ConformanceTest,
        RegistryDomain::CertificationTier,
    ] {
        let _ = writeln!(
            output,
            "    pub mod {} {{",
            domain.as_str().replace('-', "_")
        );
        for entry in set.entries.iter().filter(|entry| entry.domain == domain) {
            let _ = writeln!(
                output,
                "        pub const {}: aequora_registry_types::{} = aequora_registry_types::{}({});",
                screaming_snake(&entry.name),
                rust_id_type(domain),
                rust_id_type(domain),
                entry.id
            );
        }
        output.push_str("    }\n");
    }
    output.push_str("}\n");
    output
}

fn rust_id_type(domain: RegistryDomain) -> &'static str {
    match domain {
        RegistryDomain::Entity => "EntityTypeId",
        RegistryDomain::Operation => "OperationKind",
        RegistryDomain::Event => "EventKind",
        RegistryDomain::Field => "FieldId",
        RegistryDomain::Capability => "CapabilityId",
        RegistryDomain::ConsistencyProfile => "ConsistencyProfileId",
        RegistryDomain::Error => "ErrorCode",
        RegistryDomain::Job => "JobKind",
        RegistryDomain::Consumer => "ConsumerKind",
        RegistryDomain::AuditAction => "AuditActionId",
        RegistryDomain::Migration => "MigrationId",
        RegistryDomain::Protocol => "ProtocolVersion",
        RegistryDomain::Message => "MessageKind",
        RegistryDomain::Permission => "PermissionId",
        RegistryDomain::AdminAction => "AdminActionId",
        RegistryDomain::Reason => "ReasonCode",
        RegistryDomain::DecisionRule => "DecisionRuleId",
        RegistryDomain::ArtifactFormat => "ArtifactFormatId",
        RegistryDomain::ConformanceProfile => "ConformanceProfileId",
        RegistryDomain::ConformanceTest => "ConformanceTestId",
        RegistryDomain::CertificationTier => "CertificationTierId",
    }
}

fn screaming_snake(name: &str) -> String {
    let mut output = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_uppercase() && index > 0 {
            output.push('_');
        }
        output.push(character.to_ascii_uppercase());
    }
    output
}

#[must_use]
pub fn registry_digest(set: &RegistrySet) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&set.manifest.generation.to_le_bytes());
    for entry in &set.entries {
        hasher.update(semantic_digest(entry).as_bytes());
        hasher.update(format!("{:?}", entry.status).as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

#[must_use]
pub fn generated_markdown(set: &RegistrySet) -> String {
    let mut output = format!(
        "# Aequora durable registry\n\nGeneration: `{}`\n\nDigest: `{}`\n\n| Domain | ID | Name | Status | Owner | Schema | Description |\n|---|---:|---|---|---|---:|---|\n",
        set.manifest.generation,
        registry_digest(set)
    );
    for entry in &set.entries {
        let schema = entry
            .schema_version
            .map_or_else(|| "-".into(), |value| value.to_string());
        let _ = writeln!(
            output,
            "| {} | {} | {} | {:?} | {} | {} | {} |",
            entry.domain,
            entry.id,
            markdown_cell(&entry.name),
            entry.status,
            markdown_cell(&entry.owner.team),
            schema,
            markdown_cell(&entry.description)
        );
    }
    output
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "\\|")
}

/// Writes or checks a deterministic artifact.
///
/// # Errors
///
/// Returns I/O or drift failures.
pub fn artifact(path: &Path, content: &str, check: bool) -> Result<(), CodegenError> {
    if check {
        if read(path)? != content {
            return Err(CodegenError::Drift(path.to_path_buf()));
        }
        return Ok(());
    }
    fs::write(path, content).map_err(|source| CodegenError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Reads the committed lockfile.
///
/// # Errors
///
/// Returns I/O or RON parse errors.
pub fn load_lock(root: &Path) -> Result<RegistryLock, CodegenError> {
    let path = root.join("registry/registry.lock");
    ron::from_str(&read(&path)?).map_err(|error| CodegenError::Ron {
        path,
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_registry_types::{OwnerRef, RegistryRef};

    fn entry(domain: RegistryDomain, id: u32, name: &str) -> RegistryEntry {
        RegistryEntry {
            domain,
            id,
            name: name.into(),
            owner: OwnerRef {
                team: "team".into(),
                crate_name: "crate".into(),
                module: "module".into(),
            },
            status: SupportStatus::Current,
            introduced_in: "0.1.0".into(),
            description: "Stable semantics.".into(),
            schema_version: None,
            supported_schema_versions: Vec::new(),
            attributes: BTreeMap::new(),
            references: Vec::new(),
            security_sensitive: false,
            change_proposal_ref: None,
            migration_ref: None,
            replacement: None,
        }
    }

    fn set(entries: Vec<RegistryEntry>) -> RegistrySet {
        RegistrySet {
            manifest: RegistryManifest {
                generation: 1,
                release: "0.1.0".into(),
                namespace: "aequora-core".into(),
            },
            entries,
        }
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let value = entry(RegistryDomain::AuditAction, 1, "FirstAction");
        let mut duplicate = value.clone();
        duplicate.name = "SecondAction".into();
        assert!(matches!(
            validate(&set(vec![value, duplicate])),
            Err(RegistryError::DuplicateId { .. })
        ));
    }

    #[test]
    fn unresolved_reference_is_rejected() {
        let mut value = entry(RegistryDomain::AuditAction, 1, "FirstAction");
        value.references.push(RegistryRef {
            domain: RegistryDomain::Permission,
            id: 99,
        });
        assert!(matches!(
            validate(&set(vec![value])),
            Err(RegistryError::UnresolvedReference { .. })
        ));
    }

    #[test]
    fn removed_published_id_cannot_be_reused() {
        let old = entry(RegistryDomain::AuditAction, 1, "FirstAction");
        let lock = make_lock(&set(vec![old]));
        let replacement = entry(RegistryDomain::AuditAction, 1, "DifferentAction");
        assert!(matches!(
            verify_lock(&set(vec![replacement]), &lock),
            Err(RegistryError::PublishedIdChanged { .. })
        ));
    }

    #[test]
    fn required_safety_capability_cannot_fallback() {
        let mut value = entry(RegistryDomain::Capability, 1, "AuthorityEpochV1");
        value
            .attributes
            .insert("requirement".into(), "RequiredForSafety".into());
        value
            .attributes
            .insert("fallback".into(), "Continue".into());
        assert!(matches!(
            validate(&set(vec![value])),
            Err(RegistryError::InvalidEntry { .. })
        ));
    }

    #[test]
    fn schema_version_regression_is_reported_breaking() {
        let mut old_entry = entry(RegistryDomain::ArtifactFormat, 1, "SnapshotV1");
        old_entry.schema_version = Some(2);
        old_entry.supported_schema_versions = vec![1, 2];
        let mut new_entry = old_entry.clone();
        new_entry.schema_version = Some(1);
        let changes = diff(&set(vec![old_entry]), &set(vec![new_entry]));
        assert_eq!(
            changes[0].classification,
            ChangeClassification::BreakingWithMigration
        );
        assert_eq!(changes[0].summary, "schema version regressed");
    }

    #[test]
    fn missing_owner_is_rejected() {
        let mut value = entry(RegistryDomain::AuditAction, 1, "FirstAction");
        value.owner.team.clear();
        assert!(matches!(
            validate(&set(vec![value])),
            Err(RegistryError::InvalidEntry { .. })
        ));
    }

    #[test]
    fn semantic_digest_frames_attribute_delimiters_and_replacement() {
        let mut combined = entry(RegistryDomain::AuditAction, 1, "FirstAction");
        combined.attributes.insert("a".into(), "b|c=d".into());
        let mut split = entry(RegistryDomain::AuditAction, 1, "FirstAction");
        split.attributes.insert("a".into(), "b".into());
        split.attributes.insert("c".into(), "d".into());
        assert_ne!(semantic_digest(&combined), semantic_digest(&split));

        let before = semantic_digest(&combined);
        combined.replacement = Some(RegistryRef {
            domain: RegistryDomain::AuditAction,
            id: 2,
        });
        assert_ne!(semantic_digest(&combined), before);
    }

    #[test]
    fn control_characters_and_unbounded_metadata_are_rejected() {
        let mut value = entry(RegistryDomain::AuditAction, 1, "FirstAction");
        value.description = "unsafe\nmarkdown".into();
        assert!(matches!(
            validate(&set(vec![value])),
            Err(RegistryError::InvalidEntry { .. })
        ));
    }
}
