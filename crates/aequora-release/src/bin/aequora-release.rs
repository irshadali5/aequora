use aequora_release::{ArtifactHashes, PromotionEvidence, SupportMatrix};
use sha2::{Digest as _, Sha256};
use std::{
    env,
    fs::File,
    io::{self, Read},
    path::Path,
    process::ExitCode,
};

const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("aequora-release: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(mut arguments: impl Iterator<Item = String>) -> Result<String, String> {
    match arguments.next().as_deref() {
        Some("hash") => {
            let path = required_path(arguments.next(), "hash <artifact>")?;
            let (size, hashes) = hash_file(&path).map_err(|error| error.to_string())?;
            Ok(format!(
                "name={} size={} sha256={} blake3={}",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| "artifact name is not UTF-8".to_owned())?,
                size,
                hex(&hashes.sha256),
                hex(&hashes.blake3),
            ))
        }
        Some("support-matrix") => {
            let path = required_path(arguments.next(), "support-matrix <matrix.ron>")?;
            let text = read_metadata(&path)?;
            let matrix = SupportMatrix::from_ron(&text).map_err(|error| error.to_string())?;
            Ok(format!("support-matrix: ok targets={}", matrix.targets.len()))
        }
        Some("verify-promotion") => {
            let path = required_path(
                arguments.next(),
                "verify-promotion <evidence.ron> <minimum-approvers>",
            )?;
            let minimum = arguments
                .next()
                .ok_or_else(|| {
                    "usage: aequora-release verify-promotion <evidence.ron> <minimum-approvers>"
                        .to_owned()
                })?
                .parse::<usize>()
                .map_err(|_| "minimum approvers must be an integer".to_owned())?;
            let evidence: PromotionEvidence = ron::from_str(&read_metadata(&path)?)
                .map_err(|error| format!("promotion evidence is malformed: {error}"))?;
            evidence
                .authorize_stable(minimum)
                .map_err(|error| error.to_string())?;
            Ok(format!(
                "promotion: authorized release={} approvers={}",
                evidence.release_id,
                evidence.approvers.len()
            ))
        }
        _ => Err(
            "usage: aequora-release hash <artifact> | support-matrix <matrix.ron> | verify-promotion <evidence.ron> <minimum-approvers>"
                .to_owned(),
        ),
    }
}

fn required_path(value: Option<String>, usage: &str) -> Result<std::path::PathBuf, String> {
    value
        .map(Into::into)
        .ok_or_else(|| format!("usage: aequora-release {usage}"))
}

fn read_metadata(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(u64::try_from(MAX_METADATA_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Err("metadata exceeds the 8 MiB limit".to_owned());
    }
    String::from_utf8(bytes).map_err(|_| "metadata must be UTF-8".to_owned())
}

fn hash_file(path: &Path) -> io::Result<(u64, ArtifactHashes)> {
    let mut file = File::open(path)?;
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other("artifact size overflow"))?;
        sha256.update(&buffer[..count]);
        blake3.update(&buffer[..count]);
    }
    Ok((
        total,
        ArtifactHashes {
            sha256: sha256.finalize().into(),
            blake3: *blake3.finalize().as_bytes(),
        },
    ))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}
