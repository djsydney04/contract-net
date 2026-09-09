use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct ReferenceCase {
    pub task_type: String,
    pub params: Value,
    pub expected: String,
}

pub fn reference_cases() -> Vec<ReferenceCase> {
    serde_json::from_str(include_str!("../fixtures/reference.json"))
        .expect("checked-in reference fixtures must be valid")
}
