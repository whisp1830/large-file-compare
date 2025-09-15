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

#[derive(Clone, serde::Serialize)]
pub struct DiffLine {
    pub line_number: usize,
    pub text: String,
}

#[derive(Clone, serde::Serialize)]
pub struct ModifiedLine {
    pub line_a: DiffLine,
    pub line_b: DiffLine,
}