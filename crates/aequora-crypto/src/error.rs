use thiserror::Error;

/// Stable canonical encoding failure.
#[derive(Debug, Error)]
pub enum CanonicalError {
    #[error("canonical encoding failed")]
    Encode(#[source] postcard::Error),
}

/// Invalid or insecure cryptographic policy.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PolicyError {
    #[error("the cryptographic policy has an empty allowed algorithm set")]
    EmptyAlgorithmSet,
    #[error("the selected digest algorithm is disallowed")]
    DigestDisallowed,
    #[error("the selected signature algorithm is disallowed")]
    SignatureDisallowed,
    #[error("the selected encryption algorithm is disallowed")]
    EncryptionDisallowed,
    #[error("a required feature is not supported by the peer")]
    RequiredCapabilityMissing,
}

/// Signing-key lookup or signing failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SigningError {
    #[error("signing key is unknown")]
    KeyUnknown,
    #[error("key purpose does not match the requested operation")]
    PurposeMismatch,
    #[error("the key is not active for new signatures")]
    KeyNotActive,
    #[error("cryptographically secure randomness is unavailable")]
    RandomnessUnavailable,
    #[error("canonical signing input is invalid")]
    CanonicalEncoding,
    #[error("key provider is temporarily unavailable")]
    ProviderUnavailable,
    #[error("key provider denied the requested operation")]
    ProviderDenied,
}

/// Fail-closed signature, artifact, or trust verification failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum VerificationError {
    #[error("artifact digest does not match its canonical content")]
    DigestMismatch,
    #[error("signature is invalid")]
    SignatureInvalid,
    #[error("verification key is unknown")]
    KeyUnknown,
    #[error("verification key is revoked for this signature time")]
    KeyRevoked,
    #[error("verification key was not valid at the signature time")]
    KeyExpired,
    #[error("key purpose does not authorize this artifact")]
    PurposeMismatch,
    #[error("algorithm is disallowed by current policy")]
    AlgorithmDisallowed,
    #[error("artifact tenant does not match the trust context")]
    TenantMismatch,
    #[error("artifact type does not match the trust context")]
    ArtifactTypeMismatch,
    #[error("canonical verification input is invalid")]
    CanonicalEncoding,
    #[error("device identity does not match the registered signing key")]
    DeviceMismatch,
    #[error("the device signing key is not acceptable")]
    DeviceKeyUnavailable,
}

/// Authenticated-encryption or key-provider failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum EncryptionError {
    #[error("encryption key is unknown")]
    KeyUnknown,
    #[error("encryption key purpose does not match")]
    PurposeMismatch,
    #[error("the key is not active for new encryption")]
    KeyNotActive,
    #[error("encryption algorithm is disallowed")]
    AlgorithmDisallowed,
    #[error("ciphertext authentication or associated-data verification failed")]
    DecryptFailed,
    #[error("associated-data version is unsupported")]
    AssociatedDataVersion,
    #[error("cryptographically secure randomness is unavailable")]
    RandomnessUnavailable,
    #[error("canonical associated data is invalid")]
    CanonicalEncoding,
    #[error("password key derivation failed")]
    KeyDerivationFailed,
    #[error("key provider is temporarily unavailable")]
    ProviderUnavailable,
    #[error("key provider denied the requested operation")]
    ProviderDenied,
}

/// Key registry trust and monotonicity failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RegistryError {
    #[error("key registry generation rollback was rejected")]
    Rollback,
    #[error("key registry contains duplicate key identity")]
    DuplicateKey,
    #[error("key registry key metadata is invalid")]
    InvalidKey,
    #[error("key registry signature is invalid")]
    SignatureInvalid,
    #[error("key registry root is unknown")]
    RootUnknown,
    #[error("canonical registry input is invalid")]
    CanonicalEncoding,
}

/// Key destruction or rotation safety failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum KeyLifecycleError {
    #[error("required ciphertext still depends on this key")]
    RequiredCiphertextRemains,
    #[error("a usable recovery or backup key copy remains")]
    UsableKeyCopyRemains,
    #[error("a plaintext cache remains")]
    PlaintextCacheRemains,
    #[error("key status transition is invalid")]
    InvalidTransition,
    #[error("key destruction authorization evidence is missing")]
    AuthorizationEvidenceMissing,
}

/// Invalid use of an opaque client-managed payload.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProtectedPayloadError {
    #[error("server-readable domain responsibilities are incompatible with opaque E2E payloads")]
    ServerRequiresPlaintext,
    #[error("protected payload structure is invalid")]
    InvalidEnvelope,
    #[error("key epoch must advance across a membership change")]
    EpochDidNotAdvance,
}

/// Invalid high-level cryptography service composition.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CryptoBuildError {
    #[error("cryptographic policy is invalid")]
    InvalidPolicy,
    #[error("the policy requires a signing provider")]
    SigningProviderRequired,
    #[error("the policy requires an encryption provider")]
    EncryptionProviderRequired,
}
