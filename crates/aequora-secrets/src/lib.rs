//! Provider-neutral secret references and short-lived redacting values.
//!
//! Ordinary configuration contains [`SecretRef`] values, never plaintext credentials. Resolution
//! happens at a composition root through a [`SecretResolver`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, fmt, fs, path::PathBuf, sync::Arc};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Stable, non-secret lookup key understood by a secret provider.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SecretKey(String);

impl SecretKey {
    /// Creates a non-empty provider key.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError::InvalidReference`] for an empty or whitespace-only key.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SecretError::InvalidReference);
        }
        Ok(Self(value))
    }

    /// Returns the non-secret provider key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reference to one named provider and key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecretProviderRef {
    /// Provider registered at the composition root.
    pub provider: String,
    /// Provider-specific non-secret lookup key.
    pub key: SecretKey,
}

/// Serializable reference to secret material. This type never contains plaintext secret bytes.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub enum SecretRef {
    /// Name of an environment variable.
    Environment(SecretKey),
    /// Mounted credential file. Diagnostics deliberately do not print this path.
    File(PathBuf),
    /// Key in a platform secure store.
    OsStore(SecretKey),
    /// Key in an application-registered provider.
    Provider(SecretProviderRef),
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment(key) => formatter.debug_tuple("Environment").field(key).finish(),
            Self::File(_) => formatter.write_str("File(<redacted-path>)"),
            Self::OsStore(key) => formatter.debug_tuple("OsStore").field(key).finish(),
            Self::Provider(reference) => {
                formatter.debug_tuple("Provider").field(reference).finish()
            }
        }
    }
}

/// How a consumer can adopt a rotated value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RotationClass {
    /// A new value can be resolved and swapped without replacing the consumer.
    Reloadable,
    /// The consuming pool, listener, or client must be replaced.
    ConsumerReplacement,
    /// The process must restart to adopt the value safely.
    RestartRequired,
}

/// Plaintext secret bytes with redacting diagnostics and zeroization on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Wraps bytes returned directly by a trusted provider.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Exposes bytes only at the final secret-consuming boundary.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

/// UTF-8 secret with redacting diagnostics and zeroization on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    /// Wraps a string returned directly by a trusted provider.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Exposes the value only at the final secret-consuming boundary.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

/// Provider-specific secret resolution boundary.
#[async_trait]
pub trait SecretProvider: Send + Sync {
    /// Resolves one key without caching or logging the plaintext value.
    ///
    /// # Errors
    ///
    /// Returns a typed provider error without embedding secret contents.
    async fn resolve(&self, key: &SecretKey) -> Result<SecretBytes, SecretError>;
}

/// Built-in provider for simple container and CI environment injection.
///
/// High-assurance deployments should prefer an OS store or dedicated external provider.
pub struct EnvironmentSecretProvider;

#[async_trait]
impl SecretProvider for EnvironmentSecretProvider {
    async fn resolve(&self, key: &SecretKey) -> Result<SecretBytes, SecretError> {
        let value = env::var(key.as_str()).map_err(|error| match error {
            env::VarError::NotPresent => SecretError::NotFound,
            env::VarError::NotUnicode(_) => SecretError::ProviderFailure,
        })?;
        if value.is_empty() {
            return Err(SecretError::NotFound);
        }
        Ok(SecretBytes::new(value.into_bytes()))
    }
}

/// Resolution error that never contains plaintext secret material.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SecretError {
    /// A reference was empty or malformed.
    #[error("invalid secret reference")]
    InvalidReference,
    /// No matching provider was registered.
    #[error("secret provider is unavailable")]
    ProviderUnavailable,
    /// The provider could not find the requested key.
    #[error("secret was not found")]
    NotFound,
    /// A mounted secret file did not meet the local security policy.
    #[error("secret file is insecure or unreadable")]
    InsecureFile,
    /// The provider rejected or failed the request.
    #[error("secret provider failed")]
    ProviderFailure,
}

/// Composition-root registry for environment, file, OS-store, and external providers.
#[derive(Default)]
pub struct SecretResolver {
    environment: Option<Arc<dyn SecretProvider>>,
    os_store: Option<Arc<dyn SecretProvider>>,
    providers: BTreeMap<String, Arc<dyn SecretProvider>>,
}

impl SecretResolver {
    /// Creates an empty resolver that fails closed until providers are registered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an environment provider selected by [`SecretRef::Environment`].
    #[must_use]
    pub fn with_environment(mut self, provider: Arc<dyn SecretProvider>) -> Self {
        self.environment = Some(provider);
        self
    }

    /// Registers an OS secure-store provider selected by [`SecretRef::OsStore`].
    #[must_use]
    pub fn with_os_store(mut self, provider: Arc<dyn SecretProvider>) -> Self {
        self.os_store = Some(provider);
        self
    }

    /// Registers a named external provider.
    #[must_use]
    pub fn with_provider(
        mut self,
        name: impl Into<String>,
        provider: Arc<dyn SecretProvider>,
    ) -> Self {
        self.providers.insert(name.into(), provider);
        self
    }

    /// Resolves one reference and returns a short-lived redacting value.
    ///
    /// Secret files are read once. On Unix, group/other permission bits cause a fail-closed error.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError`] when the provider is unavailable, the file is insecure, or
    /// resolution fails.
    pub async fn resolve(&self, reference: &SecretRef) -> Result<SecretBytes, SecretError> {
        match reference {
            SecretRef::Environment(key) => {
                self.environment
                    .as_ref()
                    .ok_or(SecretError::ProviderUnavailable)?
                    .resolve(key)
                    .await
            }
            SecretRef::OsStore(key) => {
                self.os_store
                    .as_ref()
                    .ok_or(SecretError::ProviderUnavailable)?
                    .resolve(key)
                    .await
            }
            SecretRef::Provider(reference) => {
                self.providers
                    .get(&reference.provider)
                    .ok_or(SecretError::ProviderUnavailable)?
                    .resolve(&reference.key)
                    .await
            }
            SecretRef::File(path) => read_secret_file(path),
        }
    }
}

fn read_secret_file(path: &PathBuf) -> Result<SecretBytes, SecretError> {
    let metadata = fs::metadata(path).map_err(|_| SecretError::InsecureFile)?;
    if !metadata.is_file() || metadata.len() > 1_048_576 {
        return Err(SecretError::InsecureFile);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(SecretError::InsecureFile);
        }
    }
    let bytes = fs::read(path).map_err(|_| SecretError::InsecureFile)?;
    if bytes.is_empty() {
        return Err(SecretError::NotFound);
    }
    Ok(SecretBytes::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        task::{Context, Poll, Wake, Waker},
        thread,
    };

    struct ThreadWaker(thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => thread::park(),
            }
        }
    }

    struct MarkerProvider;

    #[async_trait]
    impl SecretProvider for MarkerProvider {
        async fn resolve(&self, key: &SecretKey) -> Result<SecretBytes, SecretError> {
            (key.as_str() == "present")
                .then(|| SecretBytes::new(b"part43-marker-secret".to_vec()))
                .ok_or(SecretError::NotFound)
        }
    }

    #[test]
    fn provider_resolution_redacts_marker_and_fails_closed() {
        let resolver = SecretResolver::new().with_provider("vault", Arc::new(MarkerProvider));
        let reference = SecretRef::Provider(SecretProviderRef {
            provider: "vault".to_owned(),
            key: SecretKey::new("present").unwrap_or_else(|error| panic!("{error}")),
        });
        let secret =
            block_on(resolver.resolve(&reference)).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(secret.expose(), b"part43-marker-secret");
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert!(!format!("{secret:?}").contains("marker"));

        let missing =
            SecretRef::OsStore(SecretKey::new("missing").unwrap_or_else(|error| panic!("{error}")));
        assert!(matches!(
            block_on(resolver.resolve(&missing)),
            Err(SecretError::ProviderUnavailable)
        ));
    }

    #[test]
    fn secret_strings_are_not_serializable_and_debug_is_redacted() {
        let secret = SecretString::new("part43-marker-secret".to_owned());
        assert_eq!(secret.expose(), "part43-marker-secret");
        assert_eq!(format!("{secret:?}"), "<redacted>");
        let file = SecretRef::File(PathBuf::from("/sensitive/mounted/credential"));
        assert_eq!(format!("{file:?}"), "File(<redacted-path>)");
    }
}
