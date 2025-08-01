# Billion Lines Compare

A high-performance desktop application for comparing extremely large text files, built with Tauri, Vue.js, and Rust. It's designed to handle files much larger than the available RAM, making it possible to find differences between files that are tens or even hundreds of gigabytes in size.

## Features

- **Compare Huge Files:** Select two text files of any size from your local disk.
- **View Differences:** Displays lines that are unique to each file, along with their original line numbers and occurrence counts.
- **Real-time Progress:** Monitor the comparison progress for both files.
- **Performance Metrics:** See a detailed breakdown of how long each stage of the comparison takes.

## How It Works

The application leverages the power of Rust on the backend to achieve high performance and memory efficiency.

- **Memory-Mapped Files:** Instead of loading entire files into memory, it uses `memmap2` to map files directly into virtual memory. This allows the operating system to handle paging and enables processing of files larger than the available RAM.
- **Parallel Processing:** It utilizes the `rayon` crate to process file lines across multiple CPU cores in parallel, significantly speeding up the hashing process.
- **Two-Pass Hashing Algorithm:**
    1.  **Pass 1 (Hashing & Counting):** The application reads both files in parallel. For each line, it computes a fast hash using `gxhash` and stores a count of how many times each hash appears. It also keeps an index of the first occurrence (offset and line number) of each hash. This pass identifies which lines are candidates for being unique without storing the lines themselves.
    2.  **Pass 2 (Collecting Unique Lines):** After comparing the hash counts, the application identifies the hashes that are unique to each file. It then uses the pre-built index to seek directly to the position of those unique lines in the files and reads them to display the final results.

This approach minimizes memory usage and I/O, making the comparison process extremely fast and scalable.

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
