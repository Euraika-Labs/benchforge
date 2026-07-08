use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub source: Option<String>,
}
