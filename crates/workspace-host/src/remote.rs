//! Remote access: other paired devices drive this host over the network.
#[cfg(target_os = "macos")]
pub mod bonjour;
pub(crate) mod devices;
