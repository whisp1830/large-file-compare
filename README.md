<!-- Language: -->
[English](#billion-lines-compare) | [中文](#billion-lines-compare-1)

# Billion Lines Compare

A high-performance desktop application for comparing extremely large text files, built with Tauri, Vue.js, and Rust. It's designed to handle files much larger than the available RAM, making it possible to find differences between files that are tens or even hundreds of gigabytes in size.

## 特性

- **比较巨大文件：** 从本地磁盘选择任意大小的两个文本文件。
- **查看差异：** 显示每个文件独有的行，以及其原始行号和出现次数。
- **实时进度：** 监控两个文件的比较进度。
- **性能指标：** 查看比较过程每个阶段所用时间的详细分析。
- **多种比较模式：** 可选择外部排序（适用于大于内存的文件）或内存比较（适用于较小的文件）。
- **行出现次数处理：** 可选择忽略重复行或计算出现次数。
- **主键匹配：** 基于正则表达式模式提取和匹配行，以识别修改的记录。
- **正则表达式过滤：** 使用正则表达式过滤结果中的行。
- **多语言支持：** 界面支持英语、中文、日语和韩语。

## 功能

- **比较巨大文件：** 从本地磁盘选择任意大小的两个文本文件。
- **查看差异：** 显示每个文件独有的行，以及其原始行号和出现次数。
- **实时进度：** 监控两个文件的比较进度。
- **性能指标：** 查看比较过程每个阶段所用时间的详细分析。
- **多种比较模式：** 可选择外部排序（适用于大于内存的文件）或内存比较（适用于较小的文件）。
- **行出现次数处理：** 可选择忽略重复行或计算出现次数。
- **主键匹配：** 基于正则表达式模式提取和匹配行，以识别修改的记录。
- **正则表达式过滤：** 使用正则表达式过滤结果中的行。
- **多语言支持：** 界面支持英语、中文、日语和韩语。

## 它是如何工作的

该应用程序在后端利用Rust的力量，结合外部排序算法，实现高性能和内存效率。这使得它能够比较远大于可用系统内存的文件。

核心过程分为外部排序模式下的三个主要阶段：

1.  **并行哈希和外部排序（映射阶段）：**
    *   应用程序在单独的线程中并发处理两个输入文件。
    *   对于每个文件，它使用内存映射（`memmap2`）来避免将整个文件加载到RAM中。
    *   文件通过`rayon`分块并行处理。对于每一行，使用`gxhash`计算快速哈希，并创建`(hash, original_offset)`对。
    *   这些对使用`extsort`库进行排序，该库执行高效的外部排序。这意味着它可以将大于RAM的数据集排序，通过将排序块溢出到磁盘上的临时文件中，然后合并它们。
    *   此阶段的结果是两个临时文件，每个输入文件一个，包含按数字排序的所有行哈希。

2.  **合并和比较（归约阶段）：**
    *   同时读取两个排序哈希文件，并逐行比较它们的哈希值。
    *   通过比较排序流，应用程序可以有效地识别差异。如果一个文件中的哈希值在另一个文件中没有匹配的哈希值，则它是唯一的。如果一个哈希值在每个文件中出现的次数不同，则将其余数计为唯一。
    *   此阶段生成指向原始文件中唯一行的偏移量列表。

3.  **收集唯一行：**
    *   使用上一步生成的唯一偏移量列表，应用程序再次使用内存映射访问原始文件。
    *   它直接定位到唯一行的特定偏移量来读取其内容。
    *   然后将行文本、其原始行号和出现次数发送到前端进行显示。

### 架构概述

应用程序具有模块化架构：

- **前端 (`src/`):** 使用 Vue 3 和 TypeScript 构建，提供用户界面
  - `App.vue`: 包含 UI 逻辑的主 Vue 组件
  - `i18n.ts`: 国际化支持
  - 使用 Tauri 插件进行文件对话框和存储管理

- **后端 (`src-tauri/src/`):** 使用 Rust 构建，处理核心比较逻辑
  - `main.rs`: 入口点，定义 Tauri 命令
  - `external/`: 包含外部排序实现
    - `comparison.rs`: 主比较逻辑
    - `file_processing.rs`: 文件处理和分区
  - `internal/`: 包含内存比较实现
  - `payloads.rs`: 进程间通信的数据结构

基于排序的方法通过将完整文件内容保留在磁盘上，并仅在内存或临时排序文件中处理轻量级哈希和偏移量来最小化内存使用。这使得比较过程极快且可扩展。

## 推荐的 IDE 设置

- [VS Code](https://code.visualstudio.com/)
- [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar)
- [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 入门 (开发)

要开始开发，请克隆存储库并安装依赖项。

```bash
# 安装前端依赖
npm install

# 以开发模式运行应用程序
npm run tauri dev
```

## 构建生产版本

要构建生产版本的应用程序：

```bash
# 构建前端
npm run build

# 构建 Tauri 应用程序（生成可执行文件）
npm run tauri build
```

构建的可执行文件将放置在 `releases/` 目录下，遵循语义化版本控制（例如 `releases/v1.0.0/`）。

## 配置选项

应用程序提供多种比较选项：

- **外部排序：** 使用高级外部排序算法（推荐用于大文件）或内存比较（用于小文件）
- **忽略重复：** 将重复行视为单个出现而不是计数
- **单线程：** 顺序处理文件而不是并行处理
- **忽略行号：** 跳过行号跟踪以加快处理速度
- **主键正则：** 提取行的特定部分以进行高级匹配
- **排除正则：** 过滤匹配特定模式的行

## 项目结构

```
BillionCompare/
├── src/                    # Vue.js 前端
│   ├── App.vue            # 主应用程序组件
│   └── i18n.ts            # 国际化定义
├── src-tauri/             # Rust 后端
│   ├── src/
│   │   ├── external/      # 外部排序实现
│   │   ├── internal/      # 内存比较实现
│   │   ├── main.rs        # Tauri 入口点和命令
│   │   └── payloads.rs    # 前后端通信的数据结构
│   ├── Cargo.toml         # Rust 依赖
│   └── tauri.conf.json    # Tauri 配置
├── public/                # 静态资源
├── releases/              # 构建的可执行文件（未提交）
└── dist/                  # 构建的前端资源
```

# Billion Lines Compare

A high-performance desktop application for comparing extremely large text files, built with Tauri, Vue.js, and Rust. It's designed to handle files much larger than the available RAM, making it possible to find differences between files that are tens or even hundreds of gigabytes in size.

## Features

- **Compare Huge Files:** Select two text files of any size from your local disk.
- **View Differences:** Displays lines that are unique to each file, along with their original line numbers and occurrence counts.
- **Real-time Progress:** Monitor the comparison progress for both files.
- **Performance Metrics:** See a detailed breakdown of how long each stage of the comparison takes.
- **Multiple Comparison Modes:** Choose between external sorting (for files larger than RAM) or in-memory comparison (for smaller files).
- **Line Occurrence Handling:** Option to ignore duplicate lines or count occurrences.
- **Primary Key Matching:** Extract and match lines based on regex patterns to identify modified records.
- **Filtering with Regex:** Exclude lines from results using regex patterns.
- **Multi-language Support:** UI available in English, Chinese, Japanese, and Korean.

## How It Works

The application leverages the power of Rust on the backend, combined with an external sort-based algorithm, to achieve high performance and memory efficiency. This allows it to compare files that are significantly larger than the available system RAM.

The core process is broken down into three main stages in the external sorting mode:

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

### Architecture Overview

The application has a modular architecture:

- **Frontend (`src/`):** Built with Vue 3 and TypeScript, providing the user interface
  - `App.vue`: Main Vue component containing UI logic
  - `i18n.ts`: Internationalization support
  - Uses Tauri plugins for file dialogs and store management

- **Backend (`src-tauri/src/`):** Built with Rust, handling the core comparison logic
  - `main.rs`: Entry point, defines Tauri commands
  - `external/`: Contains external sorting implementation
    - `comparison.rs`: Main comparison logic
    - `file_processing.rs`: File processing and partitioning
  - `internal/`: Contains in-memory comparison implementation
  - `payloads.rs`: Data structures for inter-process communication

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

## Building for Production

To build the application for production:

```bash
# Build the frontend
npm run build

# Build the Tauri application (generates executable)
npm run tauri build
```

The built executables will be placed in the `releases/` directory following semantic versioning (e.g. `releases/v1.0.0/`).

## Configuration Options

The application offers several comparison options:

- **External Sort:** Use the advanced external sorting algorithm (recommended for large files) or in-memory comparison (for smaller files)
- **Ignore Occurrences:** Treat duplicate lines as single occurrences instead of counting them
- **Single Thread:** Process files sequentially instead of in parallel
- **Ignore Line Numbers:** Skip line number tracking for faster processing
- **Primary Key Regex:** Extract specific parts of lines for advanced matching
- **Exclude Regex:** Filter out lines that match a specific pattern

## Project Structure

```
BillionCompare/
├── src/                    # Vue.js frontend
│   ├── App.vue            # Main application component
│   └── i18n.ts            # Internationalization definitions
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── external/      # External sorting implementation
│   │   ├── internal/      # In-memory comparison implementation
│   │   ├── main.rs        # Tauri entry point and commands
│   │   └── payloads.rs    # Data structures for frontend-backend communication
│   ├── Cargo.toml         # Rust dependencies
│   └── tauri.conf.json    # Tauri configuration
├── public/                # Static assets
├── releases/              # Built executables (not committed)
└── dist/                  # Built frontend assets
```
