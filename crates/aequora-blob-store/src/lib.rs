//! Content-addressed blobs remain staged until their digest is verified.
use aequora_storage_core::{LocalStorageError, PublicationState, StorageClass};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlobMetadata {
    pub digest: [u8; 32],
    pub size: u64,
    pub pinned: bool,
    pub reconstructable: bool,
    pub publication: PublicationState,
}

impl BlobMetadata {
    #[must_use]
    pub const fn storage_class(&self) -> StorageClass {
        if self.reconstructable && !self.pinned {
            StorageClass::EphemeralCache
        } else {
            StorageClass::ReplicatedState
        }
    }
    /// Verifies staged bytes and marks them verified; manifest publication remains a separate step.
    ///
    /// # Errors
    /// Returns corruption when size or BLAKE3 digest differs.
    pub fn verify(&mut self, bytes: &[u8]) -> Result<(), LocalStorageError> {
        if bytes.len() as u64 != self.size || blake3::hash(bytes).as_bytes() != &self.digest {
            return Err(LocalStorageError::Corruption);
        }
        self.publication = self.publication.transition(PublicationState::Verified)?;
        Ok(())
    }
    /// Publishes verified metadata last.
    ///
    /// # Errors
    /// Returns an error if verification has not completed.
    pub fn publish(&mut self) -> Result<(), LocalStorageError> {
        self.publication = self.publication.transition(PublicationState::Published)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bad_blob_never_publishes() {
        let mut b = BlobMetadata {
            digest: [0; 32],
            size: 1,
            pinned: false,
            reconstructable: true,
            publication: PublicationState::Staging,
        };
        assert_eq!(b.verify(b"x"), Err(LocalStorageError::Corruption));
        assert!(b.publish().is_err());
    }
}
