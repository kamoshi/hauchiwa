//! High-performance, streaming Git commit history parser for Rust.
//!
//! Adapted from the Go implementation <https://github.com/bep/gitmap> and refactored for Rust.
//! Copyright 2024 Bjørn Erik Pedersen <bjorn.erik.pedersen@gmail.com>.
//!
//! ## Example
//!
//! ```rust
//! use hauchiwa::gitmap::{Options, map};
//!
//! let opts = Options::new("main");
//! match map(opts) {
//!     Ok(repo) => {
//!         println!("Repository root: {:?}", repo.top_level_path);
//!         for (path, history) in repo.files {
//!             println!("File: {:?}, Last modified by: {}", path, history[0].author_name);
//!         }
//!     }
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;
use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, FixedOffset};
use thiserror::Error;

const GIT_EXEC: &str = "git";

// --- Errors ---

#[derive(Error, Debug)]
pub enum GitMapError {
    #[error("IO operation failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("Git executable not found: {0}")]
    GitNotFound(String),

    #[error("Git command failed with status {status}: {stderr}")]
    GitCommandFailed {
        status: std::process::ExitStatus,
        stderr: String,
    },

    #[error("Failed to parse date '{input}': {source}")]
    DateParse {
        input: String,
        #[source]
        source: chrono::ParseError,
    },

    #[error("Invalid UTF-8 in git output: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("Git log entry malformed: expected {expected} fields, found {found}")]
    MalformedLogEntry { expected: usize, found: usize },

    #[error("Canonicalization of path '{path}' failed: {source}")]
    PathResolution {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// Convenience alias for return types
pub type Result<T> = std::result::Result<T, GitMapError>;

// --- Domain ---

/// Holds detailed information about a single Git commit.
#[derive(Debug, Clone)]
pub struct GitInfo {
    /// The full commit hash.
    pub hash: String,
    /// The abbreviated commit hash.
    pub abbreviated_hash: String,
    /// The commit subject line.
    pub subject: String,
    /// The name of the commit author.
    pub author_name: String,
    /// The email address of the commit author.
    pub author_email: String,
    /// The date the commit was authored.
    pub author_date: DateTime<FixedOffset>,
    /// The date the commit was committed.
    pub commit_date: DateTime<FixedOffset>,
    /// The body of the commit message.
    pub body: String,
}

/// A history of commits for a specific file, ordered from the most recent to
/// the oldest.
pub type GitHistory = Vec<Arc<GitInfo>>;

/// A map where keys are file paths (relative to repo root) and values are
/// commit histories.
pub type GitMap = HashMap<String, GitHistory>;

/// Contains the Git commit information for all files in a repository.
#[derive(Debug, Clone)]
pub struct GitRepo {
    /// The absolute path to the root of the Git repository.
    pub top_level_path: PathBuf,
    /// The map of files to their history.
    pub files: GitMap,
}

/// Configuration options for the Git log parser.
pub struct Options {
    /// The path to the Git repository. Defaults to current directory.
    pub repository: PathBuf,
    /// The Git revision to analyze (e.g., "HEAD", "main", "v1.0").
    pub revision: String,
    /// The name or path of the git executable. Defaults to "git".
    pub git_binary: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            repository: PathBuf::from("."),
            revision: "HEAD".to_string(),
            git_binary: GIT_EXEC.to_string(),
        }
    }
}

impl Options {
    pub fn new(revision: impl AsRef<str>) -> Self {
        Self {
            revision: revision.as_ref().to_string(),
            ..Default::default()
        }
    }
}

// --- Implementation ---

/// Analyzes a Git repository and returns a map of all files to their last
/// commit information. This function executes Git commands to inspect the
/// repository at a given revision, collecting details about commits that
/// modified each file.
pub fn map(opts: Options) -> Result<GitRepo> {
    // get the absolute path to the repository
    let repo_path = opts
        .repository
        .canonicalize()
        .map_err(|e| GitMapError::PathResolution {
            path: opts.repository.clone(),
            source: e,
        })?;

    let top_level_path = find_top_level(&opts.git_binary, &repo_path)?;

    let mut child = Command::new(&opts.git_binary)
        .args([
            "-c",
            "diff.renames=0",
            "-c",
            "log.showSignature=0",
            "-C",
            repo_path.to_str().unwrap_or("."),
            "log",
            "--name-only",
            "--no-merges",
            "--format=format:%x1e%H%x1f%h%x1f%s%x1f%aN%x1f%aE%x1f%ai%x1f%ci%x1f%b%x1d",
            &opts.revision,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| GitMapError::GitNotFound(opts.git_binary.clone()))?;

    let stdout = child.stdout.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Could not capture stdout")
    })?;

    let mut reader = std::io::BufReader::new(stdout);
    let mut buffer = Vec::new();
    let mut map: GitMap = HashMap::new();

    // skip the initial bytes until the first record separator (0x1e)
    reader.read_until(b'\x1e', &mut buffer)?;
    buffer.clear();

    // stream and parse each entry
    loop {
        let bytes_read = reader.read_until(b'\x1e', &mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        // the buffer now contains the record ending with 0x1e, we trim the
        // delimiter before parsing.
        let slice = if buffer.ends_with(&[0x1e]) {
            &buffer[..buffer.len() - 1]
        } else {
            &buffer[..]
        };

        if let Err(e) = parse_entry(slice, &mut map) {
            // log parsing errors but do not crash the entire process
            eprintln!("Skipping malformed entry: {}", e);
        }

        buffer.clear();
    }

    // ensure the process finished successfully
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(GitMapError::GitCommandFailed {
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(GitRepo {
        top_level_path: PathBuf::from(top_level_path),
        files: map,
    })
}

/// Helper to run `git rev-parse --show-toplevel`
fn find_top_level(binary: &str, path: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| GitMapError::GitNotFound(binary.to_string()))?;

    if !output.status.success() {
        return Err(GitMapError::GitCommandFailed {
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Parses a raw byte slice into a GitInfo struct and updates the map.
fn parse_entry(raw: &[u8], map: &mut GitMap) -> Result<()> {
    // We strictly handle UTF-8 errors here, though lossy might be safer for
    // file paths. Switching to lossy for the whole block to prevent crashing on
    // weird author names.
    let s = String::from_utf8_lossy(raw);

    let parts: Vec<&str> = s.split('\x1d').collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let meta_str = parts[0];
    let files_str = parts[1];

    let info = Arc::new(parse_git_info(meta_str)?);

    for line in files_str.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        map.entry(String::from(trimmed))
            .or_default()
            .push(Arc::clone(&info));
    }
    Ok(())
}

fn parse_git_info(entry: &str) -> Result<GitInfo> {
    let items: Vec<&str> = entry.split('\x1f').collect();

    if items.len() < 8 {
        return Err(GitMapError::MalformedLogEntry {
            expected: 8,
            found: items.len(),
        });
    }

    Ok(GitInfo {
        hash: items[0].to_string(),
        abbreviated_hash: items[1].to_string(),
        subject: items[2].to_string(),
        author_name: items[3].to_string(),
        author_email: items[4].to_string(),
        author_date: parse_date(items[5])?,
        commit_date: parse_date(items[6])?,
        body: items[7].trim().to_string(),
    })
}

fn parse_date(date_str: &str) -> Result<DateTime<FixedOffset>> {
    DateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S %z").map_err(|e| GitMapError::DateParse {
        input: date_str.to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let opts = Options::default();
        let repo = map(opts);
        assert!(repo.is_ok(), "map() should return Ok");
        let repo = repo.unwrap();
        assert!(!repo.files.is_empty(), "Repo should have files");
    }

    #[test]
    fn test_readme_history() {
        let opts = Options::default();
        let repo = map(opts).expect("map() failed");

        // README.md is a standard file that should exist
        let history = repo
            .files
            .get("README.md")
            .expect("README.md not found in git map");

        assert!(!history.is_empty(), "README.md should have history");

        let oldest = &history[history.len() - 1];
        assert_eq!(&oldest.hash, "0a5388ad9a6f4bb821106e6ca0758af186d545ac");
        assert_eq!(&oldest.author_name, "Maciej Jur");
        assert_eq!(&oldest.subject, "git init");

        let second = &history[history.len() - 2];
        assert_eq!(&second.hash, "a0f6d4173e9e4fdceb0e95555dd55fd9a32d1b10");
        assert_eq!(&second.author_name, "Maciej Jur");
        assert_eq!(&second.subject, "docs: readme");
    }

    #[test]
    fn test_nested_history() {
        let opts = Options::default();
        let repo = map(opts).expect("map() failed");

        let history = repo
            .files
            .get("src/lib.rs")
            .expect("File at src/lib.rs not found");

        assert!(!history.is_empty(), "src/lib.rs should have history");

        let oldest = &history[history.len() - 1];
        assert_eq!(&oldest.hash, "0a5388ad9a6f4bb821106e6ca0758af186d545ac");
        assert_eq!(&oldest.author_name, "Maciej Jur");
        assert_eq!(&oldest.subject, "git init");

        let second = &history[history.len() - 2];
        assert_eq!(&second.hash, "c4ff0037357e3157aef262c45e7bc90b4c88fb85");
        assert_eq!(&second.author_name, "Maciej Jur");
        assert_eq!(&second.subject, "handle arbitrary scss and js");
    }

    #[test]
    fn test_revision() {
        let opts = Options::new("HEAD");
        let repo = map(opts);
        assert!(repo.is_ok());
    }
}
