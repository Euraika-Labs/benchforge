use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SandboxLevel {
    DryRun,
    IsolatedWorkspace,
    Docker,
    Vm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub level: SandboxLevel,
    pub network: String,
    pub timeout_seconds: u64,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            level: SandboxLevel::Docker,
            network: "provider-only".to_string(),
            timeout_seconds: 900,
        }
    }
}
