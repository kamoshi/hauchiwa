---
title: Advanced architecture
order: 5
---

# Advanced architecture

This section is for those who want to understand what's happening under the hood.

## Content-Addressable Storage (CAS)

Hauchiwa eschews traditional file overwrites in favor of a Content-Addressable
Storage system inspired by the Nix store.

When a loader processes a file, it calculates a BLAKE3 hash of the content. The
file is then stored in the cache using this hash as its identifier.

```text
.cache/hash/
  ├── a1b2c3d4e5... (style.scss)
  ├── f9e8d7c6b5... (script.js)
```

This guarantees:
1. **Deduplication**: Identical inputs always produce the same output and are
   stored once.
2. **Correctness**: We never serve stale cache data because the key *is* the
   content.

## Execution model

Hauchiwa uses a mostly static Directed Acyclic Graph (DAG) with some degree of
dynamicism enabled by the fine-grained task system.

### Diamond dependencies

A common problem in build systems is the "Diamond Dependency" issue:

```text
    A
   / \
  B   C
   \ /
    D
```

If Task D depends on both B and C, and both B and C depend on A, we can ensure A
runs exactly once. Hauchiwa's executor handles this automatically. Handles are
simple tokens that point to future results. When the executor sees multiple
tasks requesting the same Handle, it ensures the upstream task is executed only
once and the result is shared afterwards.

## Custom loaders

You are not limited to the built-in loaders. You can write your own data ingestors.

A loader is simply a task that:
1. Scans the filesystem (using `glob`).
2. Reads files.
3. Parses them into a Rust type `T`.
4. Returns a `Tracker<T>`.

Here is a simplified example of a JSON loader:

```rust
// The 'source' method creates a loader task.
// The closure handles the file reading and parsing logic.
let data_handle: Many<MyData> = config
    .task()
    .source("data/*.json")
    .run(|_ctx, _store, input| {
        // 1. Read the file content
        let content = input.read()?;
        
        // 2. Deserialize the JSON
        let data: MyData = serde_json::from_slice(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", input.path, e))?;

        // 3. Return the struct directly
        Ok(data)
    })?;
```
