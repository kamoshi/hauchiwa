# Library architecture

`hauchiwa` is a static site generator library built around a strictly typed,
parallel task graph. Unlike traditional generators that often have fixed
pipelines (e.g., "read all files" -> "process markdown" -> "render templates"),
`hauchiwa` allows users to define an arbitrary Directed Acyclic Graph (DAG) of
tasks.

## The core task graph

The heart of the library is a dependency graph where:
* **Nodes** are `Task`s: units of work that produce a specific output.
* **Edges** represent dependencies: Task A needs the output of Task B to run.

While the internal graph stores tasks dynamically (type-erased), the API exposed
to the user is strictly typed.
* A `Task` is defined by a closure or struct.
* It accepts a global context and inputs from its dependencies.
* It produces a typed output (e.g., `Vec<Page>`, `CssBundle`, `Image`).

This system ensures that if Task B depends on Task A, Task B receives exactly
the type returned by Task A. The compiler enforces this relationship.

When a task is added to the graph, the system returns a `Handle<T>`. This handle
is a lightweight reference (a token) representing the future result of that
task.

To make Task B depend on Task A:
1. Define Task A. It returns a `Handle<OutputA>`.
2. Pass that handle to Task B's definition.
3. The execution engine ensures Task A runs first, and its result is passed to
   Task B.

## Visualizing the Graph

One of the powerful features of this architecture is the ability to handle
"diamond dependencies" efficiently. This occurs when two different tasks depend
on the same shared ancestor. The execution engine guarantees the ancestor is run
exactly once, and its result is shared.

```mermaid
graph TD
    RawFiles[Loader: Read Raw Files] --> Meta[Task: Extract Metadata]
    RawFiles --> Content[Task: Parse Content]
    
    Meta --> Index[Task: Build Index Page]
    Content --> Index
    Content --> Post[Task: Build Individual Posts]
```

In this example:
1. `Loader` reads files once.
2. `Extract Metadata` and `Parse Content` both use the raw file data.
3. `Build Index Page` needs both metadata (to list dates) and content (for
   snippets).

There is no special "loading phase" in `hauchiwa`. "Loaders" are simply tasks
that happen to have **zero dependencies**. They act as the source nodes (roots)
of the graph. 

It is important to note that *any* task can have zero dependencies. Loaders are
just the most common example because they naturally don't depend on other tasks;
instead, they typically interact with the outside world (the filesystem) to
bring data into the graph.

```mermaid
graph LR
    subgraph Sources
        L1[Glob Markdown]
        L2[Glob Images]
        L3[Read Config]
    end
    
    L1 --> T1[Process Pages]
    L2 --> T1
    L3 --> T2[Generate Styles]
```

## Execution Model

The execution engine (`executor.rs`) manages the lifecycle of the build:

1. **Topological Sort:** The graph is analyzed to determine the execution order
and detect cycles.
2. **Parallel Execution:** Tasks are scheduled on a thread pool. A task is ready
to run as soon as all its dependencies have finished.
3. **Caching:** Results of tasks are cached in memory.

Because the build is a graph, `hauchiwa` can perform smart incremental builds.
When a file changes:
1. The system identifies which "Loader" task is responsible for that file.
2. It marks that task and all its descendants as "dirty".
3. Only the dirty subgraph is re-executed. Unaffected parts of the site are
    preserved.
