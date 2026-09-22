---
title: How It Works
order: 7
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

Independent ready tasks can run concurrently on the Rayon thread pool; actual
parallelism depends on available workers and the shape of your graph.

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
both B and C complete. Handles are typed graph-node identifiers. The executor keeps results in
reference-counted storage and passes borrowed values to dependent callbacks.

## Task granularity

Hauchiwa has two execution modes for tasks:

- **`One<T>`**: a task produces a single value. When invalidated, its callback
  re-runs as a whole. Reads from `Many<T>` dependencies are still tracked, so a
  `.merge()` that only reads one key can skip unrelated changes.
- **`Many<T>`**: a task produces a keyed collection. `.each().map()` can reuse
  unchanged item results. `.spread()` runs one callback to produce the collection,
  then hashes each value to detect which downstream items changed. File and
  bundle loaders rescan and process their matched entries when invalidated;
  unchanged provenance can still spare downstream mapping work.

Use `One<T>` for aggregators (sitemaps, search indexes, import maps). Use `Many<T>`
for per-file transforms (markdown -> HTML, image optimisation) where surgical
invalidation matters.

## Cache lifetime

Every `build(data)` call and the initial build of `watch(data)` execute the
whole graph. Task results and Tracker access records are retained in memory for
subsequent rebuilds within that watch session; arbitrary Rust task results are
not serialized between processes.

Disk caches serve different purposes: image conversions can reuse cached encoded
files, content-addressed assets are stored on disk, and snapshot metadata avoids
rewriting unchanged output files. A cached output file does not mean the task
that generates it is skipped on a new build.

## Content-Addressable Storage

Assets produced by `Store::save` (CSS bundles, JS bundles, images) are stored
using their BLAKE3 content hash as the filename:

```text
.cache/hash/
  a1b2c3d4e5...   (generated asset bytes)

dist/hash/
  a1b2c3d4e5.css  (served to browser)
```

This gives two guarantees:

1. **Deduplication** - identical content is stored and served once regardless of
   how many tasks produce it.
2. **Cache-busting** - when content changes, the hash changes, so browsers never
   serve stale assets from a long-lived cache.

## Dist reconciliation

After graph execution, Hauchiwa assembles a **Snapshot** containing generated
outputs, content-addressed assets, and static copies. It tracks output ownership
and content hashes for generated pages. Conflicting producers for the same output
path are reported as build errors. Shared references to the same hashed asset
are allowed.

- **Full commit**: without previous snapshot metadata, walk the output directory,
  remove files outside the snapshot, and write pages whose bytes differ on disk.
- **Diff commit**: during watch rebuilds, compare against the previous in-memory
  snapshot. Write new, changed, or missing pages and remove tracked files that
  disappeared. No full output-directory walk is needed.
- **Cold-start diff**: both `build()` and the initial watch build load persisted
  metadata when available and use it for output reconciliation. This still runs
  the whole task graph.

Metadata is saved to `{cache_dir}/snapshot/metadata.cbor` after a successful
commit. It records output paths and page hashes, not task results. Diff commits
only remove previously tracked files; unrelated files added to the output
directory are not discovered by that path. Static files and hashed assets are
materialized separately before page reconciliation.

Use `Blueprint::set_dir_dist()` and `Blueprint::set_dir_cache()` to configure
these directories. Treat the output directory as generated content.
