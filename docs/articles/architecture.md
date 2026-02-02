---
title: Architecture
order: 3
---

# Architecture

Hauchiwa's architecture is based on a few core concepts.

## Task Graph

The core of Hauchiwa is a dependency graph where nodes are **Tasks** and edges are **Dependencies**.

*   **Loaders**: Tasks that bring data into the graph (e.g., reading files).
*   **Transformers**: Tasks that process data.
*   **Aggregators**: Tasks that combine multiple inputs.

When you run a build, Hauchiwa analyzes the graph, schedules tasks in parallel, and ensures dependencies are met.

## Granularity

Hauchiwa distinguishes between:

*   **Many<T>**: A collection of items (fine-grained). Used for efficient incremental updates. If one file changes, only the relevant part of the downstream task might need to re-run (if the task supports it).
*   **One<T>**: A single unit of data (coarse-grained).

## Caching

Hauchiwa uses Content-Addressable Storage (CAS) for assets. Files are stored based on their content hash. This ensures:
1.  **Deduplication**: Identical files are stored once.
2.  **Cache-busting**: Changed content gets a new filename (perfect for long-term caching headers).
