//! Argentajo domain logic. No Tauri dependency: everything here is testable with `cargo test`.

pub mod nest;
pub mod piece;

/// Version of the `core` crate.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_the_crate_version() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
    }
}
