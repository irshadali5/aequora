use aequora_registry_codegen::{
    artifact, diff, generated_markdown, load_lock, load_registry, make_lock, verify_lock,
};
use aequora_registry_types::{AllocationClass, RegistryDomain};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

#[allow(clippy::too_many_lines)]
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".into());
    match command.as_str() {
        "verify" | "lint" => verify(Path::new(args.next().as_deref().unwrap_or(".")))?,
        "docs" => {
            let root = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
            let check = args.next().as_deref() == Some("--check");
            let set = load_registry(&root)?;
            artifact(
                &root.join("docs/registry/generated-registry.md"),
                &generated_markdown(&set),
                check,
            )?;
            println!(
                "registry docs {}",
                if check { "verified" } else { "generated" }
            );
        }
        "lock" => {
            let root = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
            let set = load_registry(&root)?;
            let content =
                ron::ser::to_string_pretty(&make_lock(&set), ron::ser::PrettyConfig::default())?;
            fs::write(root.join("registry/registry.lock"), format!("{content}\n"))?;
            println!("registry lock generated");
        }
        "explain" => {
            let root = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
            let domain = parse_domain(&args.next().ok_or("missing domain")?)?;
            let id = args.next().ok_or("missing ID")?.parse::<u32>()?;
            let set = load_registry(&root)?;
            let entry = set
                .entries
                .iter()
                .find(|entry| entry.domain == domain && entry.id == id)
                .ok_or("unknown registry ID")?;
            println!(
                "{}:{} {} [{:?}]\nowner={} crate={} module={}\n{}",
                domain,
                id,
                entry.name,
                entry.status,
                entry.owner.team,
                entry.owner.crate_name,
                entry.owner.module,
                entry.description
            );
            for (key, value) in &entry.attributes {
                println!("{key}={value}");
            }
        }
        "reserve" => {
            let root = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
            let domain = parse_domain(&args.next().ok_or("missing domain")?)?;
            let allocation = match args.next().as_deref() {
                Some("extension") => AllocationClass::RegisteredExtension,
                Some("private") => AllocationClass::VendorPrivate,
                Some("experimental") => AllocationClass::Experimental,
                Some("core") | None => AllocationClass::Core,
                Some(_) => {
                    return Err(
                        "allocation must be core, extension, private, or experimental".into(),
                    );
                }
            };
            let set = load_registry(&root)?;
            println!(
                "{}",
                next_id(
                    &set.entries
                        .iter()
                        .filter(|entry| entry.domain == domain)
                        .map(|entry| entry.id)
                        .collect::<Vec<_>>(),
                    allocation
                )
                .ok_or("ID range exhausted")?
            );
        }
        "diff" | "compatibility" => {
            let old = PathBuf::from(args.next().ok_or("missing old registry root")?);
            let new = PathBuf::from(args.next().ok_or("missing new registry root")?);
            let changes = diff(&load_registry(&old)?, &load_registry(&new)?);
            for change in &changes {
                println!(
                    "{:?} {}:{} {}",
                    change.classification, change.domain, change.id, change.summary
                );
            }
            if command == "compatibility"
                && changes.iter().any(|change| {
                    matches!(
                        change.classification,
                        aequora_registry_types::ChangeClassification::BreakingWithMigration
                            | aequora_registry_types::ChangeClassification::SecurityRequired
                    )
                })
            {
                return Err("registry contains changes requiring explicit approval and migration/security review".into());
            }
        }
        _ => usage(),
    }
    Ok(())
}

fn verify(root: &Path) -> Result<(), Box<dyn Error>> {
    let set = load_registry(root)?;
    let lock = load_lock(root)?;
    verify_lock(&set, &lock)?;
    artifact(
        &root.join("docs/registry/generated-registry.md"),
        &generated_markdown(&set),
        true,
    )?;
    println!(
        "registry verified generation={} entries={}",
        set.manifest.generation,
        set.entries.len()
    );
    Ok(())
}

fn parse_domain(value: &str) -> Result<RegistryDomain, Box<dyn Error>> {
    match value {
        "entity" => Ok(RegistryDomain::Entity),
        "operation" => Ok(RegistryDomain::Operation),
        "event" => Ok(RegistryDomain::Event),
        "field" => Ok(RegistryDomain::Field),
        "capability" => Ok(RegistryDomain::Capability),
        "profile" => Ok(RegistryDomain::ConsistencyProfile),
        "error" => Ok(RegistryDomain::Error),
        "job" => Ok(RegistryDomain::Job),
        "consumer" => Ok(RegistryDomain::Consumer),
        "audit-action" => Ok(RegistryDomain::AuditAction),
        "migration" => Ok(RegistryDomain::Migration),
        "protocol" => Ok(RegistryDomain::Protocol),
        "message" => Ok(RegistryDomain::Message),
        "permission" => Ok(RegistryDomain::Permission),
        "admin-action" => Ok(RegistryDomain::AdminAction),
        "reason" => Ok(RegistryDomain::Reason),
        "decision-rule" => Ok(RegistryDomain::DecisionRule),
        "artifact-format" => Ok(RegistryDomain::ArtifactFormat),
        _ => Err(format!("unknown registry domain `{value}`").into()),
    }
}

fn next_id(used: &[u32], allocation: AllocationClass) -> Option<u32> {
    let (start, end) = match allocation {
        AllocationClass::Core => (1, 0x3fff_ffff),
        AllocationClass::RegisteredExtension => (0x4000_0000, 0xbfff_ffff),
        AllocationClass::VendorPrivate => (0xc000_0000, 0xefff_ffff),
        AllocationClass::Experimental => (0xf000_0000, u32::MAX),
    };
    (start..=end).find(|candidate| !used.contains(candidate))
}

fn usage() {
    println!(
        "aequora-registry verify|lint [root]\n  docs [root] [--check]\n  lock [root]\n  diff|compatibility <old-root> <new-root>\n  explain [root] <domain> <id>\n  reserve [root] <domain> [core|extension|private|experimental]"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_stays_inside_allocation_range() {
        assert_eq!(next_id(&[1, 2], AllocationClass::Core), Some(3));
        assert_eq!(
            next_id(&[], AllocationClass::Experimental),
            Some(0xf000_0000)
        );
    }
}
