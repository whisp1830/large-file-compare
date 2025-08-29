<script setup lang="ts">
import {ref, watch, computed} from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

const fileAPath = ref("");
const fileBPath = ref("");
const useExternalSort = ref(true);
const ignoreOccurences = ref(true);
const useSingleThread = ref(false);
const ignoreLineNumber = ref(false);
const primaryKeyRegexEnable = ref(false);
const primaryKeyRegex = ref("");
const progressA = ref(0);
const progressB = ref(0);
const progressText = ref("Starting...");
interface DiffLine {
  line_number: number;
  text: string;
}

interface StepDetail {
  step: string;
  duration_ms: number;
}

const uniqueToA = ref<DiffLine[]>([]);
const uniqueToB = ref<DiffLine[]>([]);
const stepDetails = ref<StepDetail[]>([]);
const showDetails = ref(false);
const comparisonStarted = ref(false);
const comparisonDuration = ref<string | null>(null);

const currentLanguage = ref('en');

const translations = {
  'en': {
    title: "Large File Comparator",
    selectFileA: "Select File A",
    selectFileB: "Select File B",
    noFileSelected: "No file selected",
    useExternalSort: "use external sort",
    useExternalSortDesc: "...",
    ignoreOccurences: "ignore occurences",
    ignoreOccurencesDesc: "...",
    useSingleThread: "use single thread",
    useSingleThreadDesc: "...",
    ignoreLineNumber: "ignore line number",
    ignoreLineNumberDesc: "...",
    primaryKeyRegexLabel: "Primary Key Regex:",
    primaryKeyRegexLabelDesc: "...",
    primaryKeyRegexPlaceholder: "e.g., ^(\d+),",
    startComparison: "Start Comparison",
    comparing: "Comparing...",
    fileAProgress: "File A Progress:",
    fileBProgress: "File B Progress:",
    comparisonTime: "Comparison Time:",
    details: "Details",
    detailsLog: "Details Log:",
    uniqueInA: "Unique in File A",
    uniqueInB: "Unique in File B",
    lines: "lines",
    seconds: "seconds"
  },
  'zh': {
    title: "大文件比较器",
    selectFileA: "选择文件A",
    selectFileB: "选择文件B",
    noFileSelected: "未选择文件",
    useExternalSort: "使用外部排序",
    useExternalSortDesc: "...",
    ignoreOccurences: "忽略出现次数",
    ignoreOccurencesDesc: "...",
    useSingleThread: "使用单线程",
    useSingleThreadDesc: "...",
    ignoreLineNumber: "忽略行号",
    ignoreLineNumberDesc: "...",
    primaryKeyRegexLabel: "主键正则表达式:",
    primaryKeyRegexLabelDesc: "...",
    primaryKeyRegexPlaceholder: "例如, ^(\d+),",
    startComparison: "开始比较",
    comparing: "比较中...",
    fileAProgress: "文件A进度:",
    fileBProgress: "文件B进度:",
    comparisonTime: "比较用时:",
    details: "详情",
    detailsLog: "详细日志:",
    uniqueInA: "文件A独有",
    uniqueInB: "文件B独有",
    lines: "行",
    seconds: "秒"
  },
  'ja': {
    title: "大きなファイルの比較",
    selectFileA: "ファイルAを選択",
    selectFileB: "ファイルBを選択",
    noFileSelected: "ファイルが選択されていません",
    useExternalSort: "外部ソートを使用",
    useExternalSortDesc: "...",
    ignoreOccurences: "出現回数を無視",
    ignoreOccurencesDesc: "...",
    useSingleThread: "シングルスレッドを使用",
    useSingleThreadDesc: "...",
    ignoreLineNumber: "行番号を無視",
    ignoreLineNumberDesc: "...",
    primaryKeyRegexLabel: "主キー正規表現:",
    primaryKeyRegexLabelDesc: "...",
    primaryKeyRegexPlaceholder: "例, ^(\d+),",
    startComparison: "比較を開始",
    comparing: "比較中...",
    fileAProgress: "ファイルAの進捗:",
    fileBProgress: "ファイルBの進捗:",
    comparisonTime: "比較時間:",
    details: "詳細",
    detailsLog: "詳細ログ:",
    uniqueInA: "ファイルAのみ",
    uniqueInB: "ファイルBのみ",
    lines: "行",
    seconds: "秒"
  },
  'ko': {
    title: "대용량 파일 비교기",
    selectFileA: "파일 A 선택",
    selectFileB: "파일 B 선택",
    noFileSelected: "선택된 파일 없음",
    useExternalSort: "외부 정렬 사용",
    useExternalSortDesc: "...",
    ignoreOccurences: "발생 횟수 무시",
    ignoreOccurencesDesc: "...",
    useSingleThread: "단일 스레드 사용",
    useSingleThreadDesc: "...",
    ignoreLineNumber: "줄 번호 무시",
    ignoreLineNumberDesc: "...",
    primaryKeyRegexLabel: "기본 키 정규식:",
    primaryKeyRegexLabelDesc: "...",
    primaryKeyRegexPlaceholder: "예, ^(\d+),",
    startComparison: "비교 시작",
    comparing: "비교 중...",
    fileAProgress: "파일 A 진행률:",
    fileBProgress: "파일 B 진행률:",
    comparisonTime: "비교 시간:",
    details: "세부 정보",
    detailsLog: "세부 로그:",
    uniqueInA: "파일 A에만 있음",
    uniqueInB: "파일 B에만 있음",
    lines: "줄",
    seconds: "초"
  }
};

const t = computed(() => translations[currentLanguage.value as 'en' | 'zh' | 'ja' | 'ko']);

async function selectFile(fileVar: 'A' | 'B') {
  const selected = await open({
    multiple: false,
  });
  if (selected) {
    if (fileVar === 'A') {
      fileAPath.value = selected as string;
    } else {
      fileBPath.value = selected as string;
    }
  }
}

let startTime: number | null = null; // Variable to store the start time

async function startComparison() {
  if (!fileAPath.value || !fileBPath.value) {
    alert("Please select both files.");
    return;
  }
  comparisonStarted.value = true;
  progressA.value = 0;
  progressB.value = 0;
  uniqueToA.value = [];
  uniqueToB.value = [];
  stepDetails.value = [];
  showDetails.value = false;
  comparisonDuration.value = null; // Reset duration on new comparison
  progressText.value = "Starting...";
  startTime = Date.now(); // Record start time

  await invoke("start_comparison", {
    fileAPath: fileAPath.value,
    fileBPath: fileBPath.value,
    useExternalSort: useExternalSort.value,
    ignoreOccurences: ignoreOccurences.value,
    useSingleThread: useSingleThread.value,
    ignoreLineNumber: ignoreLineNumber.value,
    primaryKeyRegex: primaryKeyRegex.value
  });
}

listen('progress', (event) => {
  const payload = event.payload as { percentage: number; file: string, text: string };
  if (payload.file === 'A') {
    progressA.value = payload.percentage;
  } else {
    progressB.value = payload.percentage;
  }
  progressText.value = payload.text;
});

listen('unique_line', (event) => {
  const payload = event.payload as { file: string; line_number: number; text: string };
  const diffLine: DiffLine = { line_number: payload.line_number, text: payload.text };
  if (payload.file === 'A') {
    uniqueToA.value.push(diffLine);
  } else {
    uniqueToB.value.push(diffLine);
  }
});

listen('step_completed', (event) => {
  const payload = event.payload as StepDetail;
  stepDetails.value.push(payload);
});

listen('comparison_finished', () => {
  comparisonStarted.value = false; // Reset for next comparison

  if (startTime !== null) {
    const endTime = Date.now();
    const durationMs = endTime - startTime;
    const seconds = (durationMs / 1000).toFixed(2); // Format to 2 decimal places
    comparisonDuration.value = seconds;
    startTime = null; // Reset start time
  }
});

watch(primaryKeyRegexEnable, (newValue) => {
  if (!newValue) {
    primaryKeyRegex.value = "";
  }
});

</script>

<template>
  <div class="container">
    <div class="language-selector">
      <select v-model="currentLanguage">
        <option value="en">English</option>
        <option value="zh">中文</option>
        <option value="ja">日本語</option>
        <option value="ko">한국어</option>
      </select>
    </div>
    <h1>{{ t.title }}</h1>

    <div class="file-selection">
      <button @click="selectFile('A')">{{ t.selectFileA }}</button>
      <span class="file-path">{{ fileAPath || t.noFileSelected }}</span>
    </div>
    <div class="file-selection">
      <button @click="selectFile('B')">{{ t.selectFileB }}</button>
      <span class="file-path">{{ fileBPath || t.noFileSelected }}</span>
    </div>

    <div class="options-container">
      <input type="checkbox" id="useExternalSort" v-model="useExternalSort" />
      <label for="useExternalSort" class="tooltip" :data-tooltip="t.useExternalSortDesc">{{ t.useExternalSort }}</label>
      <input type="checkbox" id="ignoreOccurences" v-model="ignoreOccurences" />
      <label for="ignoreOccurences" class="tooltip" :data-tooltip="t.ignoreOccurencesDesc">{{ t.ignoreOccurences }}</label>
      <input type="checkbox" id="useSingleThread" v-model="useSingleThread" />
      <label for="useSingleThread" class="tooltip" :data-tooltip="t.useSingleThreadDesc">{{ t.useSingleThread }}</label>
      <input type="checkbox" id="ignoreLineNumber" v-model="ignoreLineNumber" />
      <label for="ignoreLineNumber" class="tooltip" :data-tooltip="t.ignoreLineNumberDesc">{{ t.ignoreLineNumber }}</label>
    </div>
    <div class="options-container">
      <input type="checkbox" id="primaryKeyRegexEnable" v-model="primaryKeyRegexEnable" />
      <label for="primaryKeyRegex" class="tooltip" :data-tooltip="t.primaryKeyRegexLabelDesc">{{ t.primaryKeyRegexLabel }}</label>
      <input type="text" id="primaryKeyRegex" v-show="primaryKeyRegexEnable"
             v-model="primaryKeyRegex" :placeholder="t.primaryKeyRegexPlaceholder" />
    </div>

    <button @click="startComparison" :disabled="comparisonStarted || !fileAPath || !fileBPath">
      {{ comparisonStarted ? t.comparing : t.startComparison }}
    </button>

    <div v-if="comparisonStarted" class="progress-container">
      <label>{{ t.fileAProgress }}</label>
      <progress :value="progressA" max="100"></progress>
      <label>{{ t.fileBProgress }}</label>
      <progress :value="progressB" max="100"></progress>
      <p>{{ progressText }}</p>
    </div>

    <div v-if="comparisonDuration" class="comparison-time">
      <h3>{{ t.comparisonTime }} {{ comparisonDuration }} {{ t.seconds }}</h3>
    </div>
    <button @click="showDetails = !showDetails">{{ t.details }}</button>
    <div v-if="showDetails && stepDetails.length" class="details-log">
      <h3>{{ t.detailsLog }}</h3>
      <pre v-for="(step, index) in stepDetails" :key="index">{{ step.step }}: {{ step.duration_ms }} ms</pre>
    </div>

    <div class="results-container">
      <div class="result-pane">
        <h2>{{ t.uniqueInA }} ({{ uniqueToA.length }} {{ t.lines }})</h2>
        <div class="diff-output">
          <pre v-for="line in uniqueToA" :key="line.line_number" class="diff-line removed"><code><span class="line-number">{{ line.line_number }}</span>- {{ line.text }}</code></pre>
        </div>
      </div>
      <div class="result-pane">
        <h2>{{ t.uniqueInB }} ({{ uniqueToB.length }} {{ t.lines }})</h2>
        <div class="diff-output">
          <pre v-for="line in uniqueToB" :key="line.line_number" class="diff-line added"><code><span class="line-number">{{ line.line_number }}</span>+ {{ line.text }}</code></pre>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tooltip {
  position: relative;
  cursor: pointer;
}

.tooltip::before {
  content: attr(data-tooltip);
  position: absolute;
  bottom: 100%;
  left: 50%;
  transform: translateX(-50%);
  margin-bottom: 5px;
  padding: 7px;
  width: max-content;
  max-width: 200px;
  border-radius: 4px;
  background-color: #333;
  color: #fff;
  font-size: 12px;
  text-align: center;
  visibility: hidden;
  opacity: 0;
  transition: opacity 0.3s;
  z-index: 1;
}

.tooltip::after {
  content: '';
  position: absolute;
  bottom: 100%;
  left: 50%;
  transform: translateX(-50%);
  margin-bottom: -5px;
  border-width: 5px;
  border-style: solid;
  border-color: #333 transparent transparent transparent;
  visibility: hidden;
  opacity: 0;
  transition: opacity 0.3s;
  z-index: 1;
}

.tooltip:hover::before,
.tooltip:hover::after {
  visibility: visible;
  opacity: 1;
}

.language-selector {
  position: absolute;
  top: 1rem;
  right: 1rem;
}

.container {
  padding: 2rem;
  text-align: center;
}

.file-selection {
  margin-bottom: 1rem;
  display: flex;
  align-items: center;
  justify-content: center;
}

.options-container {
  margin-bottom: 1rem;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 5px; /* Added for better spacing between checkbox and label */
}

.options-container label {
  margin-right: 15px; /* Added for better spacing between options */
}

.file-path {
  margin-left: 1rem;
  font-family: monospace;
  background-color: #f0f0f0;
  padding: 0.5rem;
  border-radius: 4px;
}

.progress-container {
  margin-top: 1rem;
}

.comparison-time {
  margin-top: 1rem;
}

.details-log {
  margin-top: 1rem;
  padding: 1rem;
  border: 1px solid #ccc;
  border-radius: 4px;
  background-color: #f9f9f9;
  text-align: left;
  max-height: 200px;
  overflow-y: auto;
}

.details-log pre {
  margin: 0;
  padding: 0.25rem 0;
  font-family: monospace;
  white-space: pre-wrap;
  font-size: 0.85em;
}

.results-container {
  display: flex;
  justify-content: space-around;
  margin-top: 2rem;
  gap: 1rem;
}

.result-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
}

textarea {
  width: 100%;
  height: 400px;
  border-radius: 4px;
  border: 1px solid #ccc;
  padding: 0.5rem;
  font-family: monospace;
}

.diff-output {
  background-color: #f8f9fa;
  border: 1px solid #dee2e6;
  border-radius: 4px;
  padding: 1rem;
  height: 400px;
  overflow-y: auto;
  text-align: left;
}

.diff-line {
  margin: 0;
  padding: 0.25rem 0.5rem;
  font-family: monospace;
  white-space: pre-wrap;
}

.diff-line.added {
  background-color: #e6ffed;
  color: #24292e;
}

.diff-line.removed {
  background-color: #ffeef0;
  color: #24292e;
}

.line-number {
  display: inline-block;
  width: 40px;
  color: #6a737d;
  text-align: right;
  margin-right: 1rem;
  user-select: none;
}
</style>