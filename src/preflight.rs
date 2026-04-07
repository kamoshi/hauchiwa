use std::process::{Command, Stdio};

/// A requirement that must be satisfied before the build can start.
///
/// Requirements are declared by loaders and checked by [`Website::build`](crate::Website::build)
/// and [`Website::watch`](crate::Website::watch) before any tasks execute. If any requirement
/// is not met, the build fails immediately with a clear error message.
///
/// # Example
///
/// Custom loaders can declare requirements by calling `.require()` on the underlying
/// task struct, or by adding a `fn requirements()` override to a custom `TypedFine`
/// or `TypedCoarse` implementation.
///
/// Built-in loaders declare their requirements automatically:
/// - `load_esbuild` requires the `esbuild` binary on PATH
/// - `load_svelte` requires the `deno` binary on PATH
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Requirement {
    /// A named binary that must be available on the system PATH.
    Binary(&'static str),
}

impl Requirement {
    /// Returns `true` if this requirement is currently satisfied.
    pub fn check(&self) -> bool {
        match self {
            Requirement::Binary(name) => Command::new(name)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok(),
        }
    }
}

impl std::fmt::Display for Requirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Requirement::Binary(name) => write!(f, "`{name}` binary on PATH"),
        }
    }
}
