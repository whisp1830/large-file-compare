use crate::payloads::StepDetailPayload;
use gxhash::GxHasher;
use std::hash::Hasher;
use tauri::{AppHandle, Emitter};

// Helper to emit step details to the frontend
pub fn emit_step_detail(app: &AppHandle, file_id: &str, step_name: &str, duration_ms: u128) {
    let step_label = format!("File {} - {}", file_id, step_name);
    if let Err(e) = app.emit(
        "step_completed",
        StepDetailPayload {
            step: step_label,
            duration_ms,
        },
    ) {
        eprintln!("Failed to emit step_completed event: {}", e);
    }
}

pub fn hash_line(line: &[u8]) -> u64 {
    let mut hasher = GxHasher::default();
    hasher.write(line);
    hasher.finish()
}
