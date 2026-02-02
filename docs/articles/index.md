---
title: Introduction
order: 1
---

# Introduction

## What is Hauchiwa?

Hauchiwa is a **static site generator library** for Rust. It is **not** a CLI
framework with a rigid directory structure.

Unlike traditional tools like Jekyll or Hugo that force you into a specific
folder layout (`_posts`, `layouts`, etc.), Hauchiwa gives you a set of building
blocks to construct your own build pipeline. You write a Rust binary that
defines exactly how your site is built.

### The core philosophy: "task graph"

At the heart of Hauchiwa is the **task graph**. Instead of implicitly guessing
relationships between files, Hauchiwa requires you to map dependencies
explicitly using a Directed Acyclic Graph (DAG).

* **Task A** (Load Markdown) produces data.
* **Task B** (Render HTML) depends on Task A.
* **Task C** (Compile SCSS) runs independently.

This explicit mapping allows Hauchiwa to understand exactly how data flows
through your build process.

## Why "graph-based"?

By defining your build as a graph, you unlock two massive benefits:

1. **Massive parallelism**: Since the dependencies are known upfront, Hauchiwa
   can schedule independent tasks to run simultaneously, saturating your CPU
   cores. For example, your CSS can compile at the exact same time your Markdown
   is being parsed.

2. **True incrementalism**: When a file changes, Hauchiwa calculates the "dirty"
   subgraph. It only rebuilds the tasks that depend on that specific change. If
   you change a CSS file, we don't re-render your HTML unless you explicitly
   wired them together.

## Audience

Hauchiwa is for you!
