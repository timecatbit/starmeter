/// The subscriptions and APIs on the homepage. Membership, API configuration and usage are separate facts:
/// a portal service's key is only sent for an explicit, read-only key check, and nothing here implies a paid
/// membership or a balance.
public enum Catalog {
    public struct Service: Equatable {
        public let key: String
        public let brand: String
        public let category: String
        public let url: String
        public let credential: String
        public let capability: String

        init(_ key: String, _ brand: String, _ category: String, _ url: String, _ credential: String = "", _ capability: String = "portal") {
            (self.key, self.brand, self.category, self.url, self.credential, self.capability) = (key, brand, category, url, credential, capability)
        }

        public var label: String { brand + (category == "订阅" ? " 订阅" : " API") }
    }

    /// Category identifiers stay stable; these say what each group measures.
    public static let groupLabels = ["Harness": "Harness Token 用量", "订阅": "订阅用量", "API": "API"]

    public static let services: [Service] = [
        Service("chatgpt", "ChatGPT", "订阅", "https://chatgpt.com", "", "client"),
        Service("claude-subscription", "Claude", "订阅", "https://claude.ai/settings/usage", "", "desktop-usage"),
        Service("minimax", "MiniMax", "订阅", "https://platform.minimax.io/user-center/payment/token-plan", "MINIMAX_API_KEY", "quota"),
        Service("gemini-subscription", "Gemini", "订阅", "https://gemini.google.com"),
        Service("grok-subscription", "Grok", "订阅", "https://grok.com"),
        Service("kimi-subscription", "Kimi", "订阅", "https://www.kimi.com"),
        Service("qoder-subscription", "Qoder CN", "订阅", "https://qoder.cn/account/usage", "", "app-log"),
        Service("cursor-subscription", "Cursor", "订阅", "https://cursor.com/dashboard", "", "account-usage"),
        Service("chatgpt-api", "ChatGPT", "API", "https://platform.openai.com/usage", "OPENAI_API_KEY"),
        Service("claude-api", "Claude", "API", "https://platform.claude.com/usage", "ANTHROPIC_API_KEY", "cost"),
        Service("deepseek", "DeepSeek", "API", "https://platform.deepseek.com/usage", "DEEPSEEK_API_KEY", "balance"),
        Service("minimax-api", "MiniMax", "API", "https://platform.minimax.io/user-center/payment/balance", "MINIMAX_PLATFORM_API_KEY"),
        Service("gemini-api", "Gemini", "API", "https://aistudio.google.com/usage", "GEMINI_API_KEY"),
        Service("grok", "Grok", "API", "https://console.x.ai", "XAI_MANAGEMENT_API_KEY", "balance"),
        Service("kimi-api", "Kimi", "API", "https://platform.moonshot.cn/console/account", "MOONSHOT_API_KEY", "balance"),
        Service("qoder-api", "Qoder CN", "API", "https://qoder.cn/organizations", "QODER_CN_API_KEY", "balance"),
    ]

    public static let byKey = Dictionary(uniqueKeysWithValues: services.map { ($0.key, $0) })
    public static let manualSubscriptions = services.filter { $0.category == "订阅" && $0.capability == "portal" }.map(\.key)
    public static let portalAPIs = services.filter { $0.category == "API" && $0.capability == "portal" }.map(\.key)
    public static let balanceAPIs = services.filter { $0.category == "API" && $0.capability == "balance" }.map(\.key)
    /// Each balance API's usual currency; Kimi's international site and some DeepSeek accounts report USD.
    public static let balanceCurrencies = ["deepseek": "CNY", "grok": "USD", "kimi-api": "CNY", "qoder-api": "Credits"]
    public static let currencies = ["CNY", "USD", "Credits"]
    /// The Harness whose API-equivalent cost a subscription is weighed against, for how many times over it has paid off.
    public static let subscriptionUsage = ["chatgpt": "codex", "claude-subscription": "claude", "cursor-subscription": "cursor", "gemini-subscription": "gemini"]
    /// A key alone configures these; DeepSeek can also come from OpenCodex, Grok needs a Team ID and Qoder CN an organization ID.
    public static let keyOnlyAPIs = services.filter { $0.category == "API" && !["deepseek", "grok", "qoder-api"].contains($0.key) }.map(\.key)
    public static let apiKeys = Dictionary(uniqueKeysWithValues: services.filter { !$0.credential.isEmpty }.map { ($0.key, $0.credential) })
    /// These aliases only identify API configuration; they never imply a subscription.
    public static let keyAliases = ["gemini-api": ["GOOGLE_API_KEY"]]

    public static func credentialNames(_ key: String) -> [String] {
        (apiKeys[key].map { [$0] } ?? []) + (keyAliases[key] ?? [])
    }
}
