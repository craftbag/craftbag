//! Reserved crate surface. Product modules land after bootstrap.

/// Package version from `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_nonzero() {
        assert!(!super::version().is_empty());
    }
}
