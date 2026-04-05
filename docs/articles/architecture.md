---
title: How It Works
order: 6
---

# How It Works

This page explains the internals Hauchiwa relies on. You don't need to understand
all of this to use the library, but it helps when debugging builds or designing
a large task graph.

## Execution model

When you call `website.build()`, Hauchiwa:

1. Performs a topological sort of the task graph to find a valid execution order.
2. Seeds all tasks with no dependencies onto the Rayon thread pool.
3. As each task completes, its result is placed in a cache and any downstream
   tasks that now have all their dependencies satisfied are seeded onto the pool.
4. The main thread waits for all tasks to finish, collecting results and timing data.

This means independent tasks always run in parallel automatically - you get full
CPU utilisation without writing any async code.

### Diamond dependencies

When two tasks share a common upstream dependency, that upstream task runs exactly
once and its result is shared:

```text
    A  (load markdown)
   / \
  B   C  (render HTML, build index)
   \ /
    D  (generate sitemap)
```

Task A is executed once. B and C run in parallel once A finishes. D runs after
both B and C complete. Handles are reference-counted pointers to the cached result,
so sharing is zero-copy.

## Task granularity

Hauchiwa has two execution modes for tasks:

- **`One<T>` (coarse-grained)**: the task runs once and produces a single value.
  If any of its dependencies change, the whole task re-runs.
- **`Many<T>` (fine-grained)**: the task runs once per item in a collection.
  If only one item changes, only that item's subtask re-runs - the rest are served
  from cache.

Use `One<T>` for aggregators (sitemaps, search indexes, import maps). Use `Many<T>`
for per-file transforms (markdown -> HTML, image optimisation) where surgical
invalidation matters.

## Content-Addressable Storage

Assets produced by `Store::save` (CSS bundles, JS bundles, images) are stored
using their BLAKE3 content hash as the filename:

```text
.cache/hash/
  a1b2c3d4e5...   (cached source)

dist/hash/
  a1b2c3d4e5.css  (served to browser)
```

This gives two guarantees:

1. **Deduplication** - identical content is stored and served once regardless of
   how many tasks produce it.
2. **Cache-busting** - when content changes, the hash changes, so browsers never
   serve stale assets from a long-lived cache.

## Dist reconciliation

After every build, Hauchiwa assembles a **Snapshot** - an in-memory record of
every file that belongs in `dist/`, which task produced it, and a BLAKE3 hash of
the content.

Two strategies are used depending on context:

- **Full commit** (first build or no previous snapshot): walks `dist/`, deletes
  any file not in the snapshot, then writes all pages whose content differs from
  what is already on disk.
- **Diff commit** (watch mode rebuild): compares the new snapshot against the
  previous one in memory. Only changed pages are written; files that disappeared
  are deleted. No `dist/` walk needed.

A slim version of the snapshot (content hashes only) is persisted to
`.cache/snapshot/metadata.cbor` after each successful build, so the diff path
is also taken on the first watch-mode rebuild after a cold `build`.
