# 排序换行符位置的必要性

**Exported:** 11/6/2025 13:14:17  
**Link:** [https://gemini.google.com/share/fd0ded21864d](https://gemini.google.com/share/fd0ded21864d)

## Prompt:
use crate::payloads::{StepDetailPayload, UniqueLinePayload};

use extsort::Sortable;

use gxhash::GxHasher;

use memmap2::Mmap;

use rayon::prelude::\*;

use std::fs::{File, OpenOptions};

use std::hash::Hasher;

use std::io::{BufWriter, Error as IoError, Read, Write};

use std::path::Path;

use std::time::Instant;

use tauri::{AppHandle, Emitter};

// Helper to emit step details to the frontend

fn emit\_step\_detail(app: &AppHandle, file\_id: &str, step\_name: &str, duration\_ms: u128) {

let step\_label = format!("File {} - {}", file\_id, step\_name);

if let Err(e) = app.emit(

"step\_completed",

StepDetailPayload {

step: step\_label,

duration\_ms,

},

) {

eprintln!("Failed to emit step\_completed event: {}", e);

}

}

#\[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Copy)\]

pub struct HashOffset(pub u64, pub u64);

// We keep the Sortable trait implementation as it provides a convenient

// binary encoding/decoding format for writing to our partition files.

impl Sortable for HashOffset {

fn encode\<W: Write\>(&self, writer: &mut W) -\> Result\<(), IoError\> {

writer.write\_all(&self.0.to\_le\_bytes())?;

writer.write\_all(&self.1.to\_le\_bytes())?;

Ok(())

}

fn decode\<R: Read\>(reader: &mut R) -\> Result\<Self, IoError\> {

let mut hash\_bytes = \[0u8; 8\];

reader.read\_exact(&mut hash\_bytes)?;

let mut offset\_bytes = \[0u8; 8\];

reader.read\_exact(&mut offset\_bytes)?;

Ok(HashOffset(

u64::from\_le\_bytes(hash\_bytes),

u64::from\_le\_bytes(offset\_bytes),

))

}

}

fn hash\_line(line: &\[u8\]) -\> u64 {

let mut hasher = GxHasher::default();

hasher.write(line);

hasher.finish()

}

fn find\_newline\_positions\_parallel(mmap: &Mmap) -\> Vec\<usize\> {

const CHUNK\_SIZE: usize = 16 \* 1024 \* 1024;

let mmap\_ptr = mmap.as\_ptr() as usize;

mmap.par\_chunks(CHUNK\_SIZE)

.flat\_map(|chunk| {

let chunk\_start\_offset = chunk.as\_ptr() as usize - mmap\_ptr;

memchr::memchr\_iter(b'\\n', chunk)

.map(move |pos| chunk\_start\_offset + pos)

.collect::\<Vec\<\_\>\>()

})

.collect()

}

pub const NUM\_PARTITIONS: u64 = 256;

/// Partitions a file into smaller files based on the hash of each line.

pub fn partition\_file(

app: &AppHandle,

input\_path: &str,

output\_dir: &Path,

progress\_file\_id: &str,

) -\> Result\<(), IoError\> {

let total\_start = Instant::now();

emit\_step\_detail(app, progress\_file\_id, "Partitioning Started", 0);

// --- Setup ---

let file = File::open(input\_path)?;

let file\_size = file.metadata()?.len();

if file\_size == 0 {

return Ok(())

}

let mmap = unsafe { Mmap::map(&file)? };

std::fs::create\_dir\_all(output\_dir)?;

// --- Find lines ---

let now = Instant::now();

let mut newline\_positions = find\_newline\_positions\_parallel(&mmap);

newline\_positions.par\_sort\_unstable(); // Sorting is needed to process lines correctly

emit\_step\_detail(app, progress\_file\_id, "Found & Sorted Newlines", now.elapsed().as\_millis());

// --- Parallel Partitioning ---

// Each thread will create a vector of HashOffsets for each partition.

// This avoids lock contention on file writers.

let now = Instant::now();

let partitioned\_data: Vec\<Vec\<HashOffset\>\> = (0..newline\_positions.len())

.into\_par\_iter()

.fold(

|| vec!\[Vec::new(); NUM\_PARTITIONS as usize\],

|mut thread\_local\_partitions, i| {

let start = if i == 0 { 0 } else { newline\_positions\[i - 1\] + 1 };

let end = newline\_positions\[i\];

let line\_bytes = &mmap\[start..end\];

let line\_bytes\_cleaned = if line\_bytes.last() == Some(&b'\\r') {

&line\_bytes\[..line\_bytes.len() - 1\]

} else {

line\_bytes

};

if !line\_bytes\_cleaned.is\_empty() {

let hash = hash\_line(line\_bytes\_cleaned);

let offset = start as u64;

let partition\_index = (hash % NUM\_PARTITIONS) as usize;

thread\_local\_partitions\[partition\_index\].push(HashOffset(hash, offset));

}

thread\_local\_partitions

},

)

.reduce(

|| vec!\[Vec::new(); NUM\_PARTITIONS as usize\],

|mut combined\_partitions, thread\_local\_partitions| {

for (i, part\_data) in thread\_local\_partitions.into\_iter().enumerate() {

combined\_partitions\[i\].extend(part\_data);

}

combined\_partitions

},

);

emit\_step\_detail(app, progress\_file\_id, "In-Memory Partitioning", now.elapsed().as\_millis());

// --- Write Partitions to Disk ---

// This is done sequentially per partition to maximize write performance.

let now = Instant::now();

partitioned\_data

.into\_par\_iter()

.enumerate()

.try\_for\_each(|(i, data)| -\> Result\<(), IoError\> {

if data.is\_empty() {

return Ok(())

}

let part\_path = output\_dir.join(format!("part\_{}", i));

let file = OpenOptions::new().write(true).create(true).truncate(true).open(part\_path)?;

let mut writer = BufWriter::new(file);

for item in data {

item.encode(&mut writer)?;

}

writer.flush()?;

Ok(())

})?;

emit\_step\_detail(app, progress\_file\_id, "Writing Partitions to Disk", now.elapsed().as\_millis());

emit\_step\_detail(app, progress\_file\_id, "Total Partitioning Time", total\_start.elapsed().as\_millis());

Ok(())

}

pub fn collect\_unique\_lines(

app: &AppHandle,

file\_path: &str,

unique\_offsets: &\[(u64, usize)\], // List of (offset, count)

file\_id: &str,

) -\> Result\<(), IoError\> {

if unique\_offsets.is\_empty() {

return Ok(())

}

let file = File::open(file\_path)?;

let mmap = unsafe { Mmap::map(&file)? };

let mut sorted\_unique\_offsets = unique\_offsets.to\_vec();

sorted\_unique\_offsets.sort\_unstable\_by\_key(|k| k.0);

let mut last\_scan\_offset: usize = 0;

let mut last\_line\_number: usize = 0;

for (offset, count) in sorted\_unique\_offsets {

let current\_offset = offset as usize;

// This logic is slow if offsets are far apart.

// A better way would be to find the line number from the offset directly

// if we had a pre-computed index of newline positions.

// For now, we keep it simple.

let newlines\_in\_between =

memchr::memchr\_iter(b'\\n', &mmap\[last\_scan\_offset..current\_offset\]).count();

let line\_number = last\_line\_number + newlines\_in\_between + 1;

let line\_end = memchr::memchr(b'\\n', &mmap\[current\_offset..\])

.map\_or(mmap.len(), |pos| current\_offset + pos);

let line\_bytes = &mmap\[current\_offset..line\_end\];

let line\_str = String::from\_utf8\_lossy(line\_bytes).trim\_end().to\_string();

let display\_line = if count \> 1 {

format!("{}\\n(x{})", line\_str, count)

} else {

line\_str

};

if let Err(e) = app.emit(

"unique\_line",

UniqueLinePayload {

file: file\_id.to\_string(),

line\_number,

text: display\_line,

},

) {

eprintln!("Failed to emit unique\_line event: {}", e);

}

last\_scan\_offset = current\_offset;

last\_line\_number = line\_number - 1;

}

Ok(())

}

newline\_positions.par\_sort\_unstable() 这一步操作有必要吗？用中文回答

## Response:
是的，`newline_positions.par_sort_unstable()` 这一步操作是**非常有必要的**，甚至是不可或缺的。

* * *

### \## 核心原因：并行处理的无序性

你的代码使用 `mmap.par_chunks(CHUNK_SIZE)` 来并行地在文件的不同“块”中查找换行符。`rayon` 库会把文件数据分成多个块，然后交给不同的 CPU核心（线程）去同时处理。

关键在于，这些线程完成任务的顺序是**不确定**的。

*   可能处理文件**后面**部分的线程会比处理文件**前面**部分的线程先完成。
*   当使用 `.collect()` 收集所有线程找到的换行符位置时，这些位置在最终的 `newline_positions` 向量 (`Vec`) 中的顺序是混乱的，而不是按照它们在文件中的先后顺序排列。

**举个例子：** 假设文件被分成两个块：

1.  **块1 (0-100字节):** 找到的换行符位置是 `[10, 50, 95]`
2.  **块2 (101-200字节):** 找到的换行符位置是 `[110, 150]`

由于并行执行的调度，处理块2的线程可能先结束，最终收集到的 `newline_positions` 向量可能是 `[110, 150, 10, 50, 95]` 这种完全无序的状态。

* * *

### \## 如果不排序会发生什么？

排序之后，下一步的代码是这样的：

```
(0..newline_positions.len())
    .into_par_iter()
    .fold(
        // ...
        |mut thread_local_partitions, i| {
            let start = if i == 0 { 0 } else { newline_positions[i - 1] + 1 };
            let end = newline_positions[i];
            let line_bytes = &mmap[start..end];
            // ...
        },
    )
```

这段代码的逻辑是：

*   第 `i` 行的结束位置是 `newline_positions[i]`。
*   第 `i` 行的开始位置是上一行结束位置的下一个字节，即 `newline_positions[i - 1] + 1`。

这个逻辑成立的**前提**是 `newline_positions` 向量中的所有位置都是**严格升序**的，即 `newline_positions[i]` 必须大于 `newline_positions[i - 1]`。

如果你跳过排序，使用一个像 `[110, 150, 10, 50, 95]` 这样的无序向量：

*   当循环到 `i = 2` 时，`newline_positions[i]` 是 `10`。
*   `newline_positions[i - 1]` 是 `150`。
*   代码会尝试计算 `start = 150 + 1 = 151` 和 `end = 10`。
*   然后它会尝试创建一个切片 `&mmap[151..10]`。

在 Rust 中，创建一个起始索引大于结束索引的切片会导致 **panic**，程序会立刻崩溃并报错。💥

* * *

### \## 总结

所以，`par_sort_unstable()` 这一步的作用是**将在并行查找中得到的无序换行符位置，重新整理成从文件头到文件尾的正确顺序**。这是保证后续代码能够正确切分每一行并处理的**关键步骤**。



---
Powered by [Gemini Exporter](https://www.geminiexporter.com)