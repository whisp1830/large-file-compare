#[derive(Clone, serde::Serialize)]
pub struct ProgressPayload {
    pub percentage: f64,
    pub file: String,
    pub text: String,
}

#[derive(Clone, serde::Serialize)]
pub struct UniqueLinePayload {
    pub file: String,
    pub line_number: usize,
    pub text: String,
}

#[derive(Clone, serde::Serialize)]
pub struct StepDetailPayload {
    pub step: String,
    pub duration_ms: u128,
}

#[derive(Clone, serde::Serialize)]
pub struct ComparisonFinishedPayload {}
