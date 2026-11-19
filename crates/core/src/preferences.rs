//! Local presentation preferences in preferences.json. No credentials or usage data live here. The file is
//! shared with older versions, so values are kept as written and read back the same way they read them.

use std::sync::LazyLock;

use crate::catalog::SERVICES;

pub struct Module {
    pub key: &'static str,
    pub label: String,
    pub category: &'static str,
}

/// Every homepage module in the order the module list shows them.
pub static MODULES: LazyLock<Vec<Module>> = LazyLock::new(|| {
    let module = |key, label: &str, category| Module { key, label: label.to_string(), category };
    let mut modules = vec![
        module("summary", "Summary", "概览"),
        module("heatmap", "用量热力图", "概览"),
        module("opencodex", "OpenCodex", "Harness"),
        module("magpie", "Magpie", "Harness"),
        module("claude", "Claude", "Harness"),
        module("codex", "ChatGPT", "Harness"),
        module("opencode", "OpenCode", "Harness"),
        module("deepseek-harness", "DeepSeek Harness", "Harness"),
        module("workbuddy", "WorkBuddy", "Harness"),
        module("qoder", "Qoder", "Harness"),
        module("gemini", "Gemini CLI", "Harness"),
        module("cursor", "Cursor", "Harness"),
    ];
    modules.extend(SERVICES.iter().map(|s| Module { key: s.key, label: s.label(), category: s.category }));
    modules.extend([
        module("sessions", "活跃会话", "其他"),
        module("komari", "Komari 监控", "其他"),
        module("updates", "Harness 更新", "其他"),
    ]);
    modules
});
