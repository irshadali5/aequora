use crate::policy::DigestAlgorithm;
use serde::{Deserialize, Serialize};

macro_rules! digest_type {
    ($name:ident, $domain:literal) => {
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub [u8; 32]);

        impl $name {
            pub const DOMAIN: &'static str = $domain;

            #[must_use]
            pub fn of(bytes: &[u8]) -> Self {
                Self(domain_digest(Self::DOMAIN, bytes))
            }

            #[must_use]
            pub const fn bytes(self) -> [u8; 32] {
                self.0
            }
        }
    };
}

digest_type!(CanonicalDigest, "AEQUORA:CANONICAL:v1");
digest_type!(SnapshotDigest, "AEQUORA:SNAPSHOT:v1");
digest_type!(OperationDigest, "AEQUORA:SIGNED-OP:v1");
digest_type!(AuditDigest, "AEQUORA:AUDIT-CHECKPOINT:v1");
digest_type!(BlobDigest, "AEQUORA:BLOB:v1");
digest_type!(ArtifactDigest, "AEQUORA:ARTIFACT-CONTENT:v1");
digest_type!(RegistryDigest, "AEQUORA:KEY-REGISTRY:v1");
digest_type!(CiphertextDigest, "AEQUORA:CIPHERTEXT:v1");
digest_type!(PlaintextDigest, "AEQUORA:PLAINTEXT:v1");

/// Computes an unambiguous BLAKE3 digest with length-framed domain separation.
#[must_use]
pub fn domain_digest(domain: &str, bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"AEQUORA:DOMAIN-DIGEST:v1\0");
    hasher.update(&(domain.len() as u64).to_le_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    *hasher.finalize().as_bytes()
}

impl From<ArtifactDigest> for CanonicalDigest {
    fn from(value: ArtifactDigest) -> Self {
        Self(value.0)
    }
}

impl CanonicalDigest {
    #[must_use]
    pub const fn algorithm() -> DigestAlgorithm {
        DigestAlgorithm::Blake3V1
    }
}
