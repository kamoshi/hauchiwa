//! Adapted from <https://github.com/bep/gitmap/blob/master/gitmap.go>
//! Copyright 2024 Bjørn Erik Pedersen <bjorn.erik.pedersen@gmail.com>.
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::str;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, FixedOffset};

pub const GIT_EXEC: &str = "git";

#[derive(Debug, Clone)]
pub struct GitRepo {
    /// Absolute path of the top-level repository directory.
    pub top_level_abs_path: String,
    /// Maps filenames to their Git commit information.
    pub files: GitMap,
}

pub type GitMap = HashMap<String, GitInfo>;

#[derive(Debug, Clone)]
pub struct GitInfo {
    pub hash: String,
    pub abbreviated_hash: String,
    pub subject: String,
    pub author_name: String,
    pub author_email: String,
    pub author_date: DateTime<FixedOffset>,
    pub commit_date: DateTime<FixedOffset>,
    pub body: String,
}

/// Options to configure the mapping.
pub struct Options {
    /// Path to the repository.
    pub repository: String,
    /// Revision to use (e.g. HEAD, branch name, etc.)
    pub revision: String,
}

/// Runs a git command with the given arguments and returns the trimmed output.
fn git(args: &[&str]) -> Result<String> {
    let output = Command::new(GIT_EXEC)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git with args {args:?}"))?;

    if !output.status.success() {
        // If git executable not found, we can check error kind.
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parses a Git log entry (separated by control characters) into a GitInfo.
fn to_git_info(entry: &str) -> Result<GitInfo> {
    let mut items: Vec<&str> = entry.split('\x1f').collect();

    // If we have 7 items, append an empty string for the body.
    if items.len() == 7 {
        items.push("");
    }
    if items.len() != 8 {
        return Err(anyhow!("unexpected number of fields in entry: {:?}", items));
    }

    // Parse the dates. The Go format "2006-01-02 15:04:05 -0700" corresponds to "%Y-%m-%d %H:%M:%S %z" in chrono.
    let author_date = DateTime::parse_from_str(items[5], "%Y-%m-%d %H:%M:%S %z")
        .with_context(|| format!("parsing author date: {}", items[5]))?;
    let commit_date = DateTime::parse_from_str(items[6], "%Y-%m-%d %H:%M:%S %z")
        .with_context(|| format!("parsing commit date: {}", items[6]))?;

    Ok(GitInfo {
        hash: items[0].to_string(),
        abbreviated_hash: items[1].to_string(),
        subject: items[2].to_string(),
        author_name: items[3].to_string(),
        author_email: items[4].to_string(),
        author_date,
        commit_date,
        body: items[7].trim().to_string(),
    })
}

/// Maps the given Git repository into a GitRepo structure.
pub fn map(opts: Options) -> Result<GitRepo> {
    let mut files: GitMap = HashMap::new();

    // Get the absolute path to the repository.
    let repo_path = Path::new(&opts.repository)
        .canonicalize()
        .with_context(|| format!("resolving repository path: {}", opts.repository))?;

    // Run "git rev-parse --show-cdup" to find how many directories to go up.
    let rev_parse_args = ["-C", &opts.repository, "rev-parse", "--show-cdup"];
    let cd_up = git(&rev_parse_args)?.trim().to_string();

    // Build the top-level path.
    let top_level_path = {
        let joined = repo_path.join(cd_up);
        // Git always returns forward slashes.
        joined
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
    };

    // Build the git log command.
    // Format string is similar to:
    //   --name-only --no-merges --format=format:\x1e%H\x1f%h\x1f%s\x1f%aN\x1f%aE\x1f%ai\x1f%ci\x1f%b\x1d <revision>
    let git_log_format = format!(
        "--name-only --no-merges --format=format:\x1e%H\x1f%h\x1f%s\x1f%aN\x1f%aE\x1f%ai\x1f%ci\x1f%b\x1d {}",
        opts.revision
    );
    // Split by whitespace (similar to Go's strings.Fields).
    let log_fields: Vec<&str> = git_log_format.split_whitespace().collect();

    // Prepend the additional git options.
    let mut args = vec![
        "-c",
        "diff.renames=0",
        "-c",
        "log.showSignature=0",
        "-C",
        &opts.repository,
        "log",
    ];
    args.extend(log_fields);

    let log_output = git(&args)?;

    // The output entries are separated by the record separator \x1e.
    // Remove extra newlines and trim the leading/trailing control characters.
    let entries_str = log_output.trim_matches(|c| c == '\n' || c == '\x1e' || c == '\'');
    if entries_str.is_empty() {
        // No entries found; return an empty GitRepo.
        return Ok(GitRepo {
            top_level_abs_path: top_level_path,
            files,
        });
    }
    // Each entry is separated by \x1e.
    for entry in entries_str.split('\x1e') {
        // Each entry consists of two parts separated by \x1d:
        // the git info and the list of filenames.
        let parts: Vec<&str> = entry.split('\x1d').collect();
        if parts.len() < 2 {
            continue;
        }
        let git_info = to_git_info(parts[0])
            .with_context(|| format!("parsing git info from entry: {:?}", parts[0]))?;
        // The second part is a newline-separated list of filenames.
        for filename in parts[1].split('\n') {
            let filename = filename.trim();
            if filename.is_empty() {
                continue;
            }
            // Only record the first commit info for the file.
            files
                .entry(filename.to_string())
                .or_insert_with(|| git_info.clone());
        }
    }

    Ok(GitRepo {
        top_level_abs_path: top_level_path,
        files,
    })
}
