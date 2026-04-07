# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).


## [0.17.0] - Unreleased

### Added
- `hauchiwa::prelude` now re-exports `Tracker`, `ImportMap`, and `Diagnostics`
- Preflight check system: loaders can declare `Requirement`s checked before any task runs; `load_esbuild` requires `esbuild` on PATH, `load_svelte` requires `deno` on PATH; missing requirements are reported together in a single error

### Changed
- **Breaking:** `Blueprint::copy_static` parameter order swapped from `(into, from)` to `(src, dest)`, matching standard Rust convention
- **Breaking:** `Website::design()` removed - use `Blueprint::new()` as the single entry point
- **Breaking:** `DocumentLoader::source()` renamed to `.glob()` for consistency with all other loaders
- **Breaking:** Glob pattern validation moved to call sites: `.glob()`, `.entry()`, and `.watch()` on all loaders now return `Result<Self, HauchiwaError>` and validate the pattern immediately; `register()` is now infallible and returns the handle directly
- **Breaking:** `IntoIterator` for `Tracker` now yields `(&str, &T)` key-value pairs, consistent with `Tracker::iter()`
- `ImportMap::register` now returns `()` instead of `&mut Self`
- `OutputBuilder::strip_prefix` now returns `Result<Self, std::path::StripPrefixError>` instead of `Result<Self, HauchiwaError>`
- `Tracker::values()` now returns `impl Iterator` instead of `Box<dyn Iterator>`, removing an unnecessary allocation
- Fixed `source_to_href` double-slash handling to only strip the leading character instead of replacing all occurrences

### Meta
- Added `#[must_use]` to `TaskDef`, `TaskBinderGlob`, `TaskBinderEach`, `TaskBinder`, `Blueprint::copy_static`, `Blueprint::finish`, and `Store::save`
- Added doc comments to `TaskDef`, `TaskBinderGlob`, `TaskBinderEach`, `TaskBinder`, and `OutputBuilder`
- `HauchiwaError::AnyhowArc` marked `#[doc(hidden)]` and its `#[from]` impl removed

## [0.16.1] - 2026-04-06

### Changed
- Copying static assets now checks modification times (mtime) before falling
  back to a full BLAKE3 content hash. This significantly speeds up the
  `copy_static` task during rebuilds by avoiding expensive I/O when files
  haven't been touched.

## [0.16.0] - 2026-04-05

### Added
- `Blueprint::copy_static` can now be used to copy arbitrary static assets to `dist/`.
- Security validation for `copy_static` target paths to ensure they stay within the `dist` directory.
- `esbuild` loader: `.external()` builder method to bundle npm packages separately and register them in the import map
- `minijinja` loader: `.filter()` builder method to register custom filters with the template environment
- `logging` feature flag: opt-in `init_logging()` sets up a tracing subscriber with ANSI colours, uptime timestamps, and progress bar integration via `tracing_indicatif`
- `Snapshot`-based dist reconciliation replaces the previous `clear_dist` + full rewrite approach: stale files are deleted by diffing the snapshot against the current `dist` tree, and unchanged pages are skipped via blake3 content hashing
- Watch mode now diffs successive snapshots in memory to avoid walking `dist` on disk; only new or changed pages are written per rebuild
- Static file copying (`copy_static`) skips unchanged files via blake3 content comparison, reducing unnecessary I/O on incremental rebuilds
- Hash assets (`Store::save`) and image loader outputs are now tracked in the snapshot, preventing them from being incorrectly deleted as stale files

### Changed
- `crate::utils::clone_static` no longer hardcodes "public" to "dist" and instead uses the paths configured in the blueprint.
- `StepCopyStatic` error type updated to an enum to handle `UnsafeTarget` errors.
- `Store::save` is now `&mut self` (previously `&self`) to support tracking saved asset paths.

## [0.15.0] - 2026-03-28

### Added
- Jinja2-style template loading via `minijinja` (new opt-in feature flag, `load_minijinja()`)
- `minijinja::context!` macro re-exported as `hauchiwa::minijinja::context` for convenience
- Rolldown-based JS/TS bundler (`load_rolldown()`) - feature flag exists but is disabled pending `rolldown` being published to crates.io

### Changed
- JavaScript loader renamed: `load_js()` → `load_esbuild()`, module path `loader::js` → `loader::esbuild`
- `Script` type moved from `loader::js::Script` to `loader::Script`

### Meta
- Added `CHANGELOG.md`

## [0.14.0] - 2026-02-10

### Added
- Conversion morphisms between `One<T>` and `Many<T>`
- Polymorphic `pagefind` loader (accepts multiple input handle types)

### Changed
- Image loader builder method renamed: `source()` → `glob()`
- Improved incremental invalidation granularity for scatter nodes
- Switched internal string representation to `Arc<str>` for cheaper cloning
- Removed `crossbeam-channel` dependency

## [0.13.0] - 2026-02-06

### Added
- Fine-grained task support: `.each().map()` for per-item processing with surgical invalidation
- Polymorphic sitemap loader

## [0.12.2] - 2026-02-02

### Changed
- Added `homepage` field to crate metadata

## [0.12.1] - 2026-02-02

### Added
- Documentation site (`docs/`)
- Dual granularity: tasks can now produce either `One<T>` or `Many<T>` output

### Fixed
- Various fixes

## [0.12.0] - 2026-02-01

### Added
- `pagefind` integration for static search indexing (`load_pagefind()`)
- `sitemap-rs` integration for sitemap generation (`load_sitemap()`)
- Tracing/logging support via `tracing`
- Image metadata persisted in CBOR cache
- Hard-linked cache for build artifacts (faster subsequent builds)
- Watch path collapsing (fewer filesystem events during development)

## [0.11.0] - 2026-01-27

### Added
- Lossy AVIF encoder support (`ImageFormat::Avif(Quality)`)

### Changed
- Refactored document loader API
- Refactored image loader

## [0.10.0] - 2026-01-24

### Changed
- Internal refactor

---

For versions prior to 0.10.0 see the [git log](https://github.com/kamoshi/hauchiwa/commits/main).
