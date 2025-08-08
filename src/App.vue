<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

const fileAPath = ref("");
const fileBPath = ref("");
const useExternalSort = ref(false);
const ignoreSequence = ref(true);
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
const comparisonDuration = ref<string | null>(null); // New reactive variable for duration

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
    ignoreSequence: ignoreSequence.value
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
    comparisonDuration.value = `${seconds} seconds`;
    startTime = null; // Reset start time
  }
});

</script>

<template>
  <div class="container">
    <h1>Large File Comparator</h1>

    <div class="file-selection">
      <button @click="selectFile('A')">Select File A</button>
      <span class="file-path">{{ fileAPath || 'No file selected' }}</span>
    </div>
    <div class="file-selection">
      <button @click="selectFile('B')">Select File B</button>
      <span class="file-path">{{ fileBPath || 'No file selected' }}</span>
    </div>

    <div class="options-container">
      <input type="checkbox" id="useExternalSort" v-model="useExternalSort" />
      <label for="useExternalSort">使用外排序 (Use External Sort)</label>
    </div>

    <button @click="startComparison" :disabled="comparisonStarted || !fileAPath || !fileBPath">
      {{ comparisonStarted ? 'Comparing...' : 'Start Comparison' }}
    </button>

    <div v-if="comparisonStarted" class="progress-container">
      <label>File A Progress:</label>
      <progress :value="progressA" max="100"></progress>
      <label>File B Progress:</label>
      <progress :value="progressB" max="100"></progress>
      <p>{{ progressText }}</p>
    </div>

    <div v-if="comparisonDuration" class="comparison-time">
      <h3>Comparison Time: {{ comparisonDuration }}</h3>
    </div>
    <button @click="showDetails = !showDetails">Details</button>
    <div v-if="showDetails && stepDetails.length" class="details-log">
      <h3>Details Log:</h3>
      <pre v-for="(step, index) in stepDetails" :key="index">{{ step.step }}: {{ step.duration_ms }} ms</pre>
    </div>

    <div class="results-container">
      <div class="result-pane">
        <h2>Unique to File A</h2>
        <div class="diff-output">
          <pre v-for="line in uniqueToA" :key="line.line_number" class="diff-line removed"><code><span class="line-number">{{ line.line_number }}</span>- {{ line.text }}</code></pre>
        </div>
      </div>
      <div class="result-pane">
        <h2>Unique to File B</h2>
        <div class="diff-output">
          <pre v-for="line in uniqueToB" :key="line.line_number" class="diff-line added"><code><span class="line-number">{{ line.line_number }}</span>+ {{ line.text }}</code></pre>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
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
