//! `ict-jig-gen`: generate ICT pogo-pin test cassettes from `EasyEDA` Gerber/Excellon packages.

pub mod board;
pub mod error;
pub mod excellon;
pub mod geom;
pub mod gerber;
pub mod numfmt;
pub mod package;
pub mod pnp;

pub use error::{Error, Result};

/// Crate name, as reported by the CLI banner.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Crate version, as reported by the CLI banner.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One-line identification string printed by the CLI.
#[must_use]
pub fn banner() -> String {
    format!("{NAME} {VERSION}")
}
