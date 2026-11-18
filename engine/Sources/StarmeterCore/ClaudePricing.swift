/// Claude's API prices per million tokens: input, output, 5-minute cache write, 1-hour cache write, cache read.
/// Source verified 2026-11-10: https://platform.claude.com/docs/en/about-claude/pricing
public enum ClaudePricing {
    public static let pricingDate = "2026-11-10"

    /// In the Python engine's order, which breaks ties between equally long names.
    static let prices: [(String, [Double])] = [
        ("claude-fable-5-2", [8, 40, 10, 16, 0.20]),
        ("claude-fable-5-1", [10, 50, 12.5, 20, 0.25]),
        ("claude-fable-5", [10, 50, 12.5, 20, 1]),
        ("claude-haiku-5-5", [0.1, 0.5, 0.125, 0.2, 0.01]),
        ("claude-opus-5-5", [4, 20, 5, 8, 0.20]),
        ("claude-sonnet-5-5", [2, 10, 2.5, 4, 0.10]),
        ("claude-opus-5", [5, 25, 6.25, 10, 0.5]),
        ("claude-opus-4-8", [5, 25, 6.25, 10, 0.5]),
        ("claude-opus-4-7", [5, 25, 6.25, 10, 0.5]),
        ("claude-opus-4-6", [5, 25, 6.25, 10, 0.5]),
        ("claude-opus-4-5", [5, 25, 6.25, 10, 0.5]),
        ("claude-sonnet-5", [2, 10, 2.5, 4, 0.2]),
        ("claude-sonnet-4-6", [3, 15, 3.75, 6, 0.3]),
        ("claude-sonnet-4-5", [3, 15, 3.75, 6, 0.3]),
        ("claude-haiku-4-5", [1, 5, 1.25, 2, 0.1]),
    ]
    /// Longest name first, so claude-fable-5-2 is tried before claude-fable-5.
    static let byLength = prices.enumerated().sorted { a, b in
        a.element.0.count != b.element.0.count ? a.element.0.count > b.element.0.count : a.offset < b.offset
    }.map(\.element)
    static let fast: Set<String> = ["claude-fable-5-2", "claude-opus-5-5", "claude-opus-5", "claude-opus-4-8"]

    /// The API-equivalent cost of one response, or nil for a model without a price.
    public static func cost(model: String, metrics: [Int64], speed: String = "standard", geo: String = "global", tier: String = "standard") -> Double? {
        cost(model: model, amounts: metrics.map(Double.init), speed: speed, geo: geo, tier: tier)
    }

    /// The same for counters that may be fractional, as Magpie's log gives them.
    public static func cost(model: String, amounts metrics: [Double], speed: String = "standard", geo: String = "global", tier: String = "standard") -> Double? {
        guard let (name, rates) = byLength.first(where: { model == $0.0 || model.hasPrefix($0.0 + "-20") }) else { return nil }
        var factor = geo == "us" || geo == "us-only" ? 1.1 : 1
        if name == "claude-haiku-5-5" && [0, 2, 3, 4].reduce(0.0, { $0 + metrics[$1] }) > 100_000 { factor *= 5 }
        if tier == "batch" { factor *= 0.5 }
        if speed == "fast" {
            guard fast.contains(name) else { return nil }
            factor *= 2
        }
        var total = 0.0
        for (count, rate) in zip(metrics, rates) { total += count * rate }
        return total * factor / 1e6
    }
}
