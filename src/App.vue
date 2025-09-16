<script setup lang="ts">
import {computed, onMounted, ref, watch} from "vue";
import {invoke} from "@tauri-apps/api/core";
import {listen} from '@tauri-apps/api/event';
import {open} from '@tauri-apps/plugin-dialog';
import {translations} from "./i18n.ts";
import {load, Store} from '@tauri-apps/plugin-store';

let store: Store
const fileAPath = ref("");
const fileBPath = ref("");
const useExternalSort = ref(true);
const ignoreOccurences = ref(true);
const useSingleThread = ref(false);
const ignoreLineNumber = ref(false);
const primaryKeyRegexEnable = ref(false);
const primaryKeyRegex = ref("");
const excludeRegexEnable = ref(false);
const excludeRegex = ref("");
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
  if (primaryKeyRegexEnable.value && !primaryKeyRegex.value) {
    alert("Please provide a primary key regex.");
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

async function exportResults() {

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

const filteredUniqueToA = computed(() => {
    if (excludeRegexEnable.value && excludeRegex.value) {
        try {
            const excludeRe = new RegExp(excludeRegex.value);
            return uniqueToA.value.filter(line => !excludeRe.test(line.text));
        } catch (e) {
            console.error("Invalid exclude regex", e);
            return uniqueToA.value;
        }
    }
    return uniqueToA.value;
});

const filteredUniqueToB = computed(() => {
    if (excludeRegexEnable.value && excludeRegex.value) {
        try {
            const excludeRe = new RegExp(excludeRegex.value);
            return uniqueToB.value.filter(line => !excludeRe.test(line.text));
        } catch (e) {
            console.error("Invalid exclude regex", e);
            return uniqueToB.value;
        }
    }
    return uniqueToB.value;
});

const pkResults = computed(() => {
  if (!primaryKeyRegexEnable.value || !primaryKeyRegex.value || !comparisonDuration.value) {
    return null;
  }

  const regex = new RegExp(primaryKeyRegex.value);
  const extractKey = (text: string): string | null => {
    const match = text.match(regex);
    return match ? (match[1] !== undefined ? match[1] : match[0]) : null;
  };

  const mapA = new Map<string, DiffLine>();
  filteredUniqueToA.value.forEach(line => {
    const key = extractKey(line.text);
    if (key !== null) {
      mapA.set(key, line);
    }
  });

  const mapB = new Map<string, DiffLine>();
  filteredUniqueToB.value.forEach(line => {
    const key = extractKey(line.text);
    if (key !== null) {
      mapB.set(key, line);
    }
  });

  const modified: { key: string, text_a: string, text_b: string, line_number_a: number, line_number_b: number }[] = [];
  const missing: DiffLine[] = [];

  for (const [key, lineA] of mapA.entries()) {
    const lineB = mapB.get(key);
    if (lineB) {
      if (lineA.text !== lineB.text) {
        modified.push({
          key: key,
          text_a: lineA.text,
          line_number_a: lineA.line_number,
          text_b: lineB.text,
          line_number_b: lineB.line_number,
        });
      }
      mapB.delete(key);
    } else {
      missing.push(lineA);
    }
  }

  const added = Array.from(mapB.values());

  return { modified, missing, added };
});

onMounted(async () => {
  store = await load('store.json');
  useExternalSort.value = await store.get('useExternalSort') ?? useExternalSort.value;
  ignoreOccurences.value = await store.get('ignoreOccurences') ?? ignoreOccurences.value;
  useSingleThread.value = await store.get('useSingleThread') ?? useSingleThread.value;
  ignoreLineNumber.value = await store.get('ignoreLineNumber') ?? ignoreLineNumber.value;
  primaryKeyRegexEnable.value = await store.get('primaryKeyRegexEnable') ?? primaryKeyRegexEnable.value;
  primaryKeyRegex.value = await store.get('primaryKeyRegex') ?? primaryKeyRegex.value;
  excludeRegexEnable.value = await store.get('excludeRegexEnable') ?? excludeRegexEnable.value;
  excludeRegex.value = await store.get('excludeRegex') ?? excludeRegex.value;
  currentLanguage.value = await store.get('currentLanguage') ?? currentLanguage.value;
  watch(primaryKeyRegexEnable, (newValue) => {
    if (!newValue) {
      primaryKeyRegex.value = "";
    }
    store.set('primaryKeyRegexEnable', newValue);
    store.save();
  });

  watch(primaryKeyRegex, (value) => {
    store.set('primaryKeyRegex', value);
    store.save();
  });

  watch(excludeRegexEnable, (newValue) => {
    if (!newValue) {
      excludeRegex.value = "";
    }
    store.set('excludeRegexEnable', newValue);
    store.save();
  });

  watch(excludeRegex, (value) => {
    store.set('excludeRegex', value);
    store.save();
  });

  watch(useExternalSort, (value) => { store.set('useExternalSort', value).then(() => store.save()); });
  watch(ignoreOccurences, (value) => { store.set('ignoreOccurences', value).then(() => store.save()); });
  watch(useSingleThread, (value) => { store.set('useSingleThread', value).then(() => store.save()); });
  watch(ignoreLineNumber, (value) => { store.set('ignoreLineNumber', value).then(() => store.save()); });
  watch(currentLanguage, (value) => { store.set('currentLanguage', value).then(() => store.save()); });
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
    <div class="options-container">
      <input type="checkbox" id="excludeRegexEnable" v-model="excludeRegexEnable" />
      <label for="excludeRegexEnable" class="tooltip" :data-tooltip="t.excludeRegexLabelDesc">{{ t.excludeRegexLabel }}</label>
      <input type="text" id="excludeRegex" v-show="excludeRegexEnable"
             v-model="excludeRegex" :placeholder="t.excludeRegexPlaceholder" />
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
      <button @click="exportResults" :disabled="!comparisonDuration">{{ t.export }}</button>
    </div>
    <button @click="showDetails = !showDetails">{{ t.details }}</button>
    <div v-if="showDetails && stepDetails.length" class="details-log">
      <h3>{{ t.detailsLog }}</h3>
      <pre v-for="(step, index) in stepDetails" :key="index">{{ step.step }}: {{ step.duration_ms }} ms</pre>
    </div>

    <div v-if="!primaryKeyRegexEnable && comparisonDuration" class="results-container">
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

    <div v-if="primaryKeyRegexEnable && pkResults" class="results-container-vertical">
      <!-- Modified Data -->
      <div class="result-pane">
        <h2>{{ t.modifiedData }} ({{ pkResults.modified.length }} {{ t.lines }})</h2>
        <div class="diff-output">
          <div v-for="line in pkResults.modified" :key="line.key" class="modified-entry">
            <pre class="diff-line removed"><code><span class="line-number">{{ line.line_number_a }}</span>- {{ line.text_a }}</code></pre>
            <pre class="diff-line added"><code><span class="line-number">{{ line.line_number_b }}</span>+ {{ line.text_b }}</code></pre>
          </div>
        </div>
      </div>
      <!-- Missing Data -->
      <div class="result-pane">
        <h2>{{ t.missingData }} ({{ pkResults.missing.length }} {{ t.lines }})</h2>
        <div class="diff-output">
          <pre v-for="line in pkResults.missing" :key="line.line_number" class="diff-line removed"><code><span class="line-number">{{ line.line_number }}</span>- {{ line.text }}</code></pre>
        </div>
      </div>
      <!-- Added Data -->
      <div class="result-pane">
        <h2>{{ t.addedData }} ({{ pkResults.added.length }} {{ t.lines }})</h2>
        <div class="diff-output">
          <pre v-for="line in pkResults.added" :key="line.line_number" class="diff-line added"><code><span class="line-number">{{ line.line_number }}</span>+ {{ line.text }}</code></pre>
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
  width: 100%;
  padding: 2rem;
  text-align: center;
  box-sizing: border-box;
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

.results-container-vertical {
  display: flex;
  flex-direction: column;
  margin-top: 2rem;
  gap: 1rem;
}

.modified-entry {
  border-bottom: 1px solid #ddd;
  padding-bottom: 0.5rem;
  margin-bottom: 0.5rem;
}
.modified-entry:last-child {
  border-bottom: none;
  padding-bottom: 0;
  margin-bottom: 0;
}

.results-container-vertical {
  display: flex;
  flex-direction: column;
  margin-top: 2rem;
  gap: 1rem;
}

.modified-entry {
  border-bottom: 1px solid #ddd;
  padding-bottom: 0.5rem;
  margin-bottom: 0.5rem;
}
.modified-entry:last-child {
  border-bottom: none;
  padding-bottom: 0;
  margin-bottom: 0;
}
</style>