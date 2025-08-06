# Billion Lines Compare

A high-performance desktop application for comparing extremely large text files, built with Tauri, Vue.js, and Rust. It's designed to handle files much larger than the available RAM, making it possible to find differences between files that are tens or even hundreds of gigabytes in size.

## Features

- **Compare Huge Files:** Select two text files of any size from your local disk.
- **View Differences:** Displays lines that are unique to each file, along with their original line numbers and occurrence counts.
- **Real-time Progress:** Monitor the comparison progress for both files.
- **Performance Metrics:** See a detailed breakdown of how long each stage of the comparison takes.

## How It Works

The application leverages the power of Rust on the backend, combined with an external sort-based algorithm, to achieve high performance and memory efficiency. This allows it to compare files that are significantly larger than the available system RAM.

The core process is broken down into three main stages:

1.  **Parallel Hashing & External Sorting (Map Phase):**
    *   The application processes both input files concurrently in separate threads.
    *   For each file, it uses memory-mapping (`memmap2`) to avoid loading the entire file into RAM.
    *   The file is processed in parallel chunks using `rayon`. For each line, a fast hash is computed using `gxhash`, and a `(hash, original_offset)` pair is created.
    *   These pairs are sorted using the `extsort` library, which performs an efficient external sort. This means it can sort datasets larger than RAM by spilling sorted chunks to temporary files on disk and then merging them.
    *   This stage results in two temporary files, one for each input file, containing all the line hashes sorted numerically.

2.  **Merge & Compare (Reduce Phase):**
    *   The two sorted hash files are read simultaneously, and their hashes are compared line by line.
    *   By comparing the sorted streams, the application can efficiently identify differences. If a hash from one file doesn't have a matching hash in the other, it's unique. If a hash appears a different number of times in each file, the surplus is counted as unique.
    *   This stage produces a list of offsets pointing to the unique lines in the original files.

3.  **Collect Unique Lines:**
    *   Using the list of unique offsets generated in the previous step, the application again uses memory-mapping to access the original files.
    *   It seeks directly to the specific offsets of the unique lines to read their content.
    *   The line text, its original line number, and its occurrence count are then sent to the frontend for display.

This sort-based approach minimizes memory usage by keeping the full file content on disk and only working with lightweight hashes and offsets in memory or in temporary sorted files. This makes the comparison process extremely fast and scalable.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/)
- [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar)
- [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Getting Started (Development)

To get started with development, clone the repository and install the dependencies.

```bash
# Install frontend dependencies
npm install

# Run the application in development mode
npm run tauri dev
```
