//! `ict-jig-gen`: generate ICT pogo-pin test cassettes from `EasyEDA` Gerber/Excellon packages.

/// Crate name, as reported by the CLI banner.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Crate version, as reported by the CLI banner.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One-line identification string printed by the CLI.
#[must_use]
pub fn banner() -> String {
    format!("{NAME} {VERSION}")
}
