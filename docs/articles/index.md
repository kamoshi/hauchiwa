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


## Why use this over X

If you are looking for a static site generator, you have likely encountered
**Zola**, **Hugo**, or **Jekyll**. These are excellent tools, but they share a
common philosophy: **Configuration over Code**.

Hauchiwa takes the opposite approach: **Code over Configuration**.

In tools like Zola, you build your site by editing a `config.toml` file. This is
great for standard blogs, but it falls apart when you have custom needs.

Hauchiwa is not a CLI tool, it is a **Rust library**. You write a standard Rust
program (`main.rs`) that defines your build pipeline. Because your configuration
is just Rust code, you can do anything Rust can do.
* Need to fetch data from the GitHub API? Just call `reqwest`.
* Need to read from a Postgres DB? Just use `sqlx`.
* Need to generate pages from a CSV file? Just use `csv`.


## The core philosophy: "task graph"

At the heart of Hauchiwa is the **task graph**. Instead of implicitly guessing
relationships between files, Hauchiwa requires you to map dependencies
explicitly using a Directed Acyclic Graph (DAG).

* **Task A** (Load Markdown) produces data.
* **Task B** (Render HTML) depends on Task A.
* **Task C** (Compile SCSS) runs independently.

This explicit mapping allows Hauchiwa to understand exactly how data flows
through your build process.

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
