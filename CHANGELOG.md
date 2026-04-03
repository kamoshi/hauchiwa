# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- `Blueprint::copy_static` can now be used to copy arbitrary static assets to `dist/`.
- Security validation for `copy_static` target paths to ensure they stay within the `dist` directory.
- `esbuild` loader: `.external()` builder method to bundle npm packages separately and register them in the import map
- `minijinja` loader: `.filter()` builder method to register custom filters with the template environment

### Changed
- `crate::utils::clone_static` no longer hardcodes "public" to "dist" and instead uses the paths configured in the blueprint.
- `StepCopyStatic` error type updated to an enum to handle `UnsafeTarget` errors.

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
