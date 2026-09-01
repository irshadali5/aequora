/// Advisory connectivity as observed by the platform bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Connectivity {
    Unknown,
    Online,
    Offline,
}

/// Advisory application lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    Foreground,
    Background,
    Suspended,
}

/// Thin platform notification forwarded to the client-facing integration boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PlatformEvent {
    Foreground,
    Background,
    Resume,
    Suspend,
    Connectivity(Connectivity),
}
