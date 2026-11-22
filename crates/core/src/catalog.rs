//! The subscriptions and APIs on the homepage. Membership, API configuration and usage are separate facts: a
//! portal service's key is only sent for an explicit, read-only key check, and nothing here implies a paid
//! membership or a balance.

pub struct Service {
    pub key: &'static str,
    pub brand: &'static str,
    pub category: &'static str,
    pub url: &'static str,
    /// The environment variable that holds its key, or "" when it takes none.
    pub credential: &'static str,
    pub capability: &'static str,
}

impl Service {
    pub fn label(&self) -> String {
        format!("{} {}", self.brand, if self.category == "订阅" { "订阅" } else { "API" })
    }
}

const fn service(
    key: &'static str,
    brand: &'static str,
    category: &'static str,
    url: &'static str,
    credential: &'static str,
    capability: &'static str,
) -> Service {
    Service { key, brand, category, url, credential, capability }
}

/// Category identifiers stay stable; these say what each group measures.
pub const GROUP_LABELS: [(&str, &str); 3] = [("Harness", "工具 Token 用量"), ("订阅", "订阅用量"), ("API", "API")];

pub const SERVICES: [Service; 16] = [
    service("chatgpt", "ChatGPT", "订阅", "https://chatgpt.com", "", "client"),
    service("claude-subscription", "Claude", "订阅", "https://claude.ai/settings/usage", "", "desktop-usage"),
    service("cursor-subscription", "Cursor", "订阅", "https://cursor.com/dashboard", "", "account-usage"),
    service("gemini-subscription", "Gemini", "订阅", "https://gemini.google.com", "", "antigravity"),
    service("grok-subscription", "Grok", "订阅", "https://grok.com", "", "portal"),
    service("kimi-subscription", "Kimi", "订阅", "https://www.kimi.com", "", "portal"),
    service(
        "minimax",
        "MiniMax",
        "订阅",
        "https://platform.minimax.io/user-center/payment/token-plan",
        "MINIMAX_API_KEY",
        "quota",
    ),
    service("qoder-subscription", "Qoder", "订阅", "https://qoder.cn/account/usage", "", "app-log"),
    service("chatgpt-api", "ChatGPT", "API", "https://platform.openai.com/usage", "OPENAI_API_KEY", "portal"),
    service("claude-api", "Claude", "API", "https://platform.claude.com/usage", "ANTHROPIC_API_KEY", "cost"),
    service("deepseek", "DeepSeek", "API", "https://platform.deepseek.com/usage", "DEEPSEEK_API_KEY", "balance"),
    service("gemini-api", "Gemini", "API", "https://aistudio.google.com/usage", "GEMINI_API_KEY", "portal"),
    service("grok", "Grok", "API", "https://console.x.ai", "XAI_MANAGEMENT_API_KEY", "balance"),
    service("kimi-api", "Kimi", "API", "https://platform.moonshot.cn/console/account", "MOONSHOT_API_KEY", "balance"),
    service(
        "minimax-api",
        "MiniMax",
        "API",
        "https://platform.minimax.io/user-center/payment/balance",
        "MINIMAX_PLATFORM_API_KEY",
        "portal",
    ),
    service("qoder-api", "Qoder", "API", "https://qoder.cn/organizations", "QODER_CN_API_KEY", "balance"),
];

pub fn by_key(key: &str) -> Option<&'static Service> {
    SERVICES.iter().find(|service| service.key == key)
}

fn keys(keep: impl Fn(&Service) -> bool) -> Vec<&'static str> {
    SERVICES.iter().filter(|service| keep(service)).map(|service| service.key).collect()
}

pub fn manual_subscriptions() -> Vec<&'static str> {
    keys(|s| s.category == "订阅" && s.capability == "portal")
}

pub fn portal_apis() -> Vec<&'static str> {
    keys(|s| s.category == "API" && s.capability == "portal")
}

pub fn balance_apis() -> Vec<&'static str> {
    keys(|s| s.category == "API" && s.capability == "balance")
}

/// A key alone configures these; DeepSeek can also come from OpenCodex, Grok needs a Team ID and Qoder CN an
/// organization ID.
pub fn key_only_apis() -> Vec<&'static str> {
    keys(|s| s.category == "API" && !["deepseek", "grok", "qoder-api"].contains(&s.key))
}

/// Each balance API's usual currency; Kimi's international site and some DeepSeek accounts report USD.
pub const BALANCE_CURRENCIES: [(&str, &str); 4] =
    [("deepseek", "CNY"), ("grok", "USD"), ("kimi-api", "CNY"), ("qoder-api", "Credits")];
pub const CURRENCIES: [&str; 3] = ["CNY", "USD", "Credits"];
/// The Harness whose API-equivalent cost a subscription is weighed against, for how many times over it has paid off.
pub const SUBSCRIPTION_USAGE: [(&str, &str); 4] = [
    ("chatgpt", "codex"),
    ("claude-subscription", "claude"),
    ("cursor-subscription", "cursor"),
    ("gemini-subscription", "gemini"),
];
/// What a module shows once it is set up, said on its settings page until it is: some APIs only check the key.
pub const SETUP_NOTES: [(&str, &str); 9] = [
    ("minimax", "需要 MiniMax Token Plan 的 Key，显示套餐各窗口的用量"),
    ("chatgpt-api", "OpenAI 没有余额接口：保存或测试时校验一次 Key，只表示 Key 可用"),
    ("claude-api", "管理员 Key 显示本月花费；个人 Key 只校验是否可用"),
    ("deepseek", "显示账户余额"),
    ("minimax-api", "保存或测试时校验一次 Key，只表示 Key 可用"),
    ("gemini-api", "Google 没有余额接口：保存或测试时校验一次 Key，只表示 Key 可用"),
    ("grok", "需要 xAI 管理 Key 和 Team ID，显示团队余额"),
    ("kimi-api", "显示账户余额（国内站与国际站都可以）"),
    ("qoder-api", "仅团队版/企业版：组织管理员的 OpenAPI Key 和组织 ID，显示组织共享的加购 Credits；个人账号没有 API"),
];
/// These aliases only identify API configuration; they never imply a subscription.
pub const KEY_ALIASES: [(&str, &[&str]); 1] = [("gemini-api", &["GOOGLE_API_KEY"])];

/// The environment variables that may hold a service's key.
pub fn credential_names(key: &str) -> Vec<&'static str> {
    let mut names: Vec<&'static str> =
        by_key(key).map(|s| s.credential).filter(|name| !name.is_empty()).into_iter().collect();
    for (alias_key, aliases) in KEY_ALIASES {
        if alias_key == key {
            names.extend_from_slice(aliases);
        }
    }
    names
}
