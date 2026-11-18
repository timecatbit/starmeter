import Foundation

extension Runtime {
    // MARK: Heatmap

    struct Cell { var slot: JSON; var label: String; var later: Bool }

    /// The heatmap's squares for a range in reading order, the number of columns and a name for each row.
    func heatmapCells(_ key: String, now: Double, zone: TimeZone, used: [String]) -> ([Cell], Int, [String]) {
        let wall = Wall(now, zone)
        let today = daysFromCivil(wall.year, wall.month, wall.day)
        func name(_ day: Int) -> String {
            let (_, m, d) = civilFromDays(day)
            return "\(m)月\(d)日 " + Runtime.weekdays[((day % 7) + 7 + 3) % 7]
        }
        if Runtime.hourlyRanges.contains(key) {
            var moments: [(Double, Int, Int)] = []  // moment, day number, hour
            var names: [String]
            if key == "24h" {
                let base = Int64((now).rounded(.down)) / 3600 * 3600 - 23 * 3600
                for i in 0..<24 {
                    let moment = Double(base + 3600 * Int64(i))
                    let local = Wall(moment, zone)
                    moments.append((moment, daysFromCivil(local.year, local.month, local.day), Int(local.seconds / 3600)))
                }
                names = ["24小时"]
            } else {
                let weekday = ((today % 7) + 7 + 3) % 7
                let back = ["today": 0, "yesterday": 1, "7d": 6, "week": weekday][key]!
                let rows = ["today": 1, "yesterday": 1, "7d": 7, "week": weekday + 1][key]!
                let first = today - back
                for d in 0..<rows {
                    let (y, m, day) = civilFromDays(first + d)
                    for h in 0..<24 {
                        var at = Wall(year: y, month: m, day: day, zone: zone)
                        at.seconds = Int64(h * 3600)
                        moments.append((at.timestamp, first + d, h))
                    }
                }
                names = ["today": ["今天"], "yesterday": ["昨天"]][key] ?? (0..<rows).map { Runtime.weekdays[(((first + $0) % 7) + 7 + 3) % 7] }
            }
            let cells = moments.map { moment, day, hour in
                Cell(slot: .int(Int64(moment.rounded(.down)) / 3600 * 3600), label: name(day) + String(format: " %02d:00", hour), later: moment > now)
            }
            return (cells, 24, names)
        }
        var first: Int, count: Int
        if key == "30d" { first = today - 29; count = 30 }
        else if key == "month" { first = daysFromCivil(wall.year, wall.month, 1); count = daysIn(wall.month, of: wall.year) }
        else {
            let start = Runtime.iso(today - (Runtime.heatmapDays - 1))
            let since = used.filter { $0 >= start }
            let span = since.min().flatMap(Runtime.dayFromISO).map { today - $0 + 1 } ?? 1
            count = Swift.min(Runtime.heatmapDays, (span + Runtime.heatmapRow - 1) / Runtime.heatmapRow * Runtime.heatmapRow)
            first = today - (count - 1)
        }
        let dates = (0..<count).map { first + $0 }
        let names: [String]
        if key == "all" {
            // A row is named for the month it starts in, once per month.
            let starts = stride(from: 0, to: count, by: Runtime.heatmapRow).map { civilFromDays(dates[$0]).1 }
            names = starts.enumerated().map { index, month in index == 0 || month != starts[index - 1] ? "\(month)月" : "" }
        } else {
            names = [key == "30d" ? "30天" : "\(wall.month)月"]
        }
        return (dates.map { Cell(slot: .string(Runtime.iso($0)), label: name($0), later: $0 > today) }, key == "all" ? Runtime.heatmapRow : count, names)
    }

    /// Days in a row with usage: the run up to today (or yesterday, while today is still empty) and the longest.
    static func streakText(_ days: Set<String>, today: Int) -> String? {
        let used = days.compactMap(dayFromISO).sorted()
        var longest = 0, run = 0
        var previous: Int?
        for day in used {
            run = previous.map { day - $0 == 1 ? run + 1 : 1 } ?? 1
            longest = Swift.max(longest, run)
            previous = day
        }
        let present = Set(used)
        var current = 0, cursor = present.contains(today) ? today : today - 1
        while present.contains(cursor) { current += 1; cursor -= 1 }
        if longest < 2 { return nil }
        return current >= 2 ? "连续 \(current) 天 · 最长 \(longest) 天" : "最长连续 \(longest) 天"
    }

    /// The squares under Summary for the selected range, counted as Summary counts them.
    func heatmapPayload(_ bundle: JSONObject, key: String, now: Double, zone: TimeZone, selected: [(String, String, JSONObject)], error: String?) -> JSON {
        let hourly = Runtime.hourlyRanges.contains(key)
        var totals = Ordered<String, (Double, Int64, Double, Int64)>()
        var partial = false
        var days = Set<String>()
        func int(_ value: JSON?) -> Int64 { Int64((Py.or(value, .int(0)).pythonFloat ?? 0).rounded(.towardZero)) }
        func float(_ value: JSON?) -> Double { Py.or(value, .int(0)).pythonFloat ?? 0 }
        let counting = Runtime.summaryUsage(selected)
        for (source, _, _) in selected {
            let part = counting[source]!
            let usage = part["usage"]?.object ?? JSONObject()
            for row in usage["days"]?.array ?? [] where float(row["totalTokens"]) > 0 || int(row["requests"]) > 0 {
                days.insert(String(Py.str(row["date"] ?? .string("")).prefix(10)))
            }
            let rows = usage[hourly ? "hours" : "days"]?.array ?? []
            // A source whose hours couldn't be read (OpenCodex's timeline) leaves the squares short.
            if hourly && rows.isEmpty && usage["summary"]?.object?["requests"]?.isTruthy == true { partial = true }
            for row in rows {
                let slot = hourly ? String(int(row["timestamp"]) / 3600 * 3600) : String(Py.str(row["date"] ?? .string("")).prefix(10))
                var t = totals[slot] ?? (0, 0, 0, 0)
                t.0 += Swift.max(0, float(row["totalTokens"])); t.1 += int(row["requests"])
                t.2 += Swift.max(0, float(row["estimatedCostUsd"])); t.3 += int(row["unpricedRequests"])
                totals[slot] = t
            }
        }
        let used = hourly ? [] : totals.items.filter { $0.1.0 > 0 || $0.1.1 > 0 }.map(\.0)
        let (cells, columns, names) = heatmapCells(key, now: now, zone: zone, used: used)
        let values = cells.map { cell -> (Double, Int64, Double, Int64) in
            let slot = cell.slot.string ?? String(cell.slot.integer ?? 0)
            return totals[slot] ?? (0, 0, 0, 0)
        }
        let status = bundle.isTruthy ? "" : (Runtime.refreshFailed(error) ? "数据暂不可用" : "待同步")
        let streak = Runtime.streakRanges.contains(key) && bundle.isTruthy ? Runtime.streakText(days, today: Runtime.dayNumber(now, zone)).map(JSON.string) ?? .null : .null
        return [
            "columns": .int(Int64(columns)), "rowLabels": .array(names.map(JSON.string)), "labels": .array(cells.map { .string($0.label) }),
            "tokens": .array(values.map { .double($0.0) }), "requests": .array(values.map { .int($0.1) }),
            "costs": .array(values.map { .double(Runtime.round6($0.2)) }), "unpriced": .array(values.map { .int($0.3) }),
            "future": .int(Int64(cells.filter(\.later).count)), "streak": streak, "status": .string(status),
            "notice": .string(partial ? "部分小时数据不可用" : ""), "hint": .string(hourly ? "每格 1 小时" : "每格 1 天"),
        ]
    }

    // MARK: The dashboard

    /// Everything the homepage draws for the range on screen. data holds the cached bundle and its error, the
    /// OpenCodex health, the exchange rate and the service statuses.
    public func dashboardPayload(_ data: JSONObject, rangeError: String? = nil, now: Double = Date().timeIntervalSince1970) -> JSONObject {
        let figure = Runtime.figure
        let zone = localZone()
        let key = data["displayed_range"]?.string.flatMap { $0.isEmpty ? nil : $0 } ?? selectedRange()
        var bundle = data["range_usage"]?.object ?? JSONObject()
        if bundle["range"] != .string(key) { bundle = JSONObject() }
        var (start, end) = rangeBounds(key, now: now, zone: zone)
        start = bundle["start"]?.number ?? start
        end = bundle["end"]?.number ?? end
        let settings = self.settings()
        let selected: [(String, String, JSONObject)] = Preferences.modules.filter { Preferences.usage.contains($0.key) && moduleEnabled($0.key) }
            .map { ($0.key, $0.label, bundle[$0.key]?.object ?? JSONObject()) }
        let usageSources = selected.map(\.2)
        let short = Runtime.shortRanges.contains(key)
        var labels: [String], span: String, period: [String]
        var slots: [JSON]
        if short {
            let first = Int64((start / 3600).rounded(.down)) * 3600, last = Int64((end / 3600).rounded(.down)) * 3600
            slots = stride(from: first, through: last, by: 3600).map(JSON.int)
            labels = stride(from: first, through: last, by: 3600).map { Runtime.format(Double($0), zone, "%H:%M") }
            if !labels.isEmpty {
                labels[0] = Runtime.format(start, zone, "%H:%M")
                labels[labels.count - 1] = Runtime.format(end, zone, "%H:%M")
            }
            span = Runtime.format(start, zone, "%m/%d %H:%M") + "–" + Runtime.format(end, zone, key == "today" ? "%H:%M" : "%m/%d %H:%M")
            period = [start, end].map { Runtime.format($0, zone, "%Y-%m-%dT%H:%M") }
            if key == "yesterday" {
                // A whole day: it ends at 24:00 and is written as its date.
                if !labels.isEmpty { labels[labels.count - 1] = "24:00" }
                let day = Runtime.dayNumber(start, zone)
                span = String(Runtime.iso(day).dropFirst(5)).replacingOccurrences(of: "-", with: "/")
                period = [Runtime.iso(day), Runtime.iso(day)]
            }
        } else {
            let last = Runtime.dayNumber(end, zone)
            let earliest = usageSources.flatMap { $0["days"]?.array ?? [] }.compactMap { day -> String? in
                guard let date = day["date"], date.isTruthy, figure(day["requests"]) > 0 || figure(day["totalTokens"]) > 0 else { return nil }
                return String(Py.str(date).prefix(10))
            }
            var first = start != 0 ? Runtime.dayNumber(start, zone) : (earliest.min().flatMap(Runtime.dayFromISO) ?? last)
            // The days the figures cover; the chart shows at most the last 366 of them.
            period = [Runtime.iso(first), Runtime.iso(last)]
            first = Swift.max(first, last - 365)
            slots = (first...Swift.max(first, last)).map { .string(Runtime.iso($0)) }
            if first > last { slots = [] }
            labels = slots.map { String($0.string!.dropFirst(5)).replacingOccurrences(of: "-", with: "/") }
            span = (labels.first ?? "") + "–" + (labels.last ?? "")
        }
        /// A model the price tables don't know yet says so; Credits are never priced.
        func priceNote(_ row: JSONObject) -> String? {
            let missing = Int64(figure(row["unpricedRequests"]))
            if missing <= 0 || figure(row["credits"]) != 0 { return nil }
            return missing >= Int64(figure(row["requests"])) ? "未定价" : "\(missing) 次未定价"
        }
        func provider(_ id: String, _ label: String, _ usage: JSONObject, _ subtitle: String, _ givenStatus: String) -> JSONObject {
            let summary = usage["summary"]?.object ?? JSONObject()
            let available = summary.isTruthy && usage["error"]?.isTruthy != true
            let total = figure(summary["totalTokens"]), models = usage["models"]?.array?.compactMap(\.object) ?? [], credits = figure(summary["credits"])
            // Qoder CN plan requests log Credits with no tokens; rank and share those models by Credits instead.
            let byCredits = total == 0 && credits != 0
            let weight: (JSONObject) -> Double = byCredits ? { figure($0["credits"]) } : { figure($0["totalTokens"]) }
            let scale = byCredits ? Swift.max(1e-9, models.reduce(0) { $0 + weight($1) }) : Swift.max(1, total)
            let rows: [JSON] = Runtime.stableSorted(models, by: { -weight($0) }).prefix(3).map { row in
                var entry: JSONObject = ["name": .string(Py.str(Py.or(row["model"], .string("未知模型")))), "tokens": .double(figure(row["totalTokens"])),
                                         "share": Runtime.capped(weight(row) / scale)]
                if figure(row["credits"]) != 0 { entry["credits"] = .double(figure(row["credits"])) }
                if let note = priceNote(row) { entry["priceNote"] = .string(note) }
                return .object(entry)
            }
            var points: [String: Double] = [:]
            for row in usage[short ? "hours" : "days"]?.array ?? [] {
                let slot = short ? String(Int64((Py.or(row["timestamp"], .int(0)).pythonFloat ?? 0).rounded(.towardZero))) : String(Py.str(row["date"] ?? .string("")).prefix(10))
                points[slot] = figure(row["totalTokens"])
            }
            let history: [JSON] = slots.map { slot in points[slot.string ?? String(slot.integer ?? 0)].map(JSON.double) ?? .int(0) }
            let inputs = figure(summary["cacheEligibleInputTokens"] ?? summary["inputTokens"])
            let cache = figure(Py.or(summary["cacheReadInputTokens"], summary["cachedInputTokens"] ?? .null))
            var cost: JSON = Runtime.summaryCostValue(summary).map { .double(figure($0)) } ?? .null
            let requests = Int64(figure(summary["requests"])), unpriced = Int64(figure(summary["unpricedRequests"]))
            if requests != 0 && unpriced >= requests { cost = .null }
            var status = givenStatus
            if !available { status = usage["error"].flatMap { $0.isTruthy ? Py.str($0) : nil } ?? (bundle.isTruthy ? "暂不可用" : "待同步") }
            else if usage["refreshError"]?.isTruthy == true { status = "刷新失败 · 保留原数据" }
            else if usage["partial"]?.isTruthy == true { status = "部分记录未读取" }
            else if unpriced >= requests && requests != 0 && credits == 0 { status = "费用待定价" }
            var card: JSONObject = ["id": .string(id), "name": .string(label)]
            for (field, value) in moduleIcon(id) { card[field] = value }
            let clients = Runtime.clientRows(usage["clients"])
            card["subtitle"] = .string(subtitle); card["status"] = .string(status); card["available"] = .bool(available)
            card["credits"] = credits != 0 ? .double(credits) : .null
            card["clients"] = clients.isEmpty ? .null : .array(clients)
            card["cost"] = cost; card["tokens"] = .double(total); card["requests"] = .int(requests); card["output"] = .double(figure(summary["outputTokens"]))
            card["cache"] = inputs != 0 ? Runtime.capped(cache / inputs) : .null
            card["coverage"] = summary["coverageRatio"].flatMap { $0.isNull ? nil : JSON.double(figure($0)) } ?? .null
            card["sessions"] = .int(Int64(figure(summary["sessions"]))); card["unpriced"] = .int(unpriced); card["models"] = .array(rows)
            card["history"] = .array(history)
            // Some requests unpriced: the cost covers only the rest. All unpriced already reads 费用待定价; OpenCodex shows
            // its 已定价 share instead.
            card["priceNote"] = 0 < unpriced && unpriced < requests && credits == 0 && id != "opencodex" ? .string("\(unpriced) 次未定价") : .null
            return card
        }
        var providers: [JSONObject] = []
        for (source, label, usage) in selected {
            var card = provider(source, label, usage, "", source == "opencodex" && data["healthz"]?.isTruthy == true ? "运行正常" : "已同步")
            // A service in trouble right now, from its official status page (服务状态 on).
            if settings["serviceStatus"]?.isTruthy == true { card["serviceNote"] = Runtime.serviceNote(data["service_status"], source).map(JSON.string) ?? .null }
            providers.append(card)
        }
        // The client's card still shows all its own records, while Summary adds what summaryUsage counts.
        let counting = Runtime.summaryUsage(selected)
        for (index, (source, label, _)) in selected.enumerated() {
            let part = counting[source]!
            let carried = part["carried"]?.object ?? JSONObject()
            if part["proxy"]?.isTruthy == true {
                providers[index]["summaryIncluded"] = false; providers[index]["subtitle"] = "未计入 Summary"
            } else if part["usage"]?.isNull ?? true {
                providers[index]["summaryIncluded"] = false; providers[index]["subtitle"] = .string("已由 \(carried.keys.joined(separator: "、")) 计入")
            } else {
                providers[index]["summaryIncluded"] = true
                if !carried.isEmpty {
                    let counted = provider(source, label, part["counted"]?.object ?? JSONObject(), "", "已同步")
                    var kept = JSONObject()
                    for field in ["cost", "tokens", "requests", "unpriced", "cache", "history"] { kept[field] = counted[field] ?? .null }
                    providers[index]["counted"] = .object(kept)
                    let total = carried.reduce(Int64(0)) { $0 + ($1.value.integer ?? 0) }
                    providers[index]["subtitle"] = .string("\(total) 次已由 \(carried.keys.joined(separator: "、")) 计入")
                }
            }
        }
        func history(_ card: JSONObject) -> [JSON] { (card["counted"]?.object ?? card)["history"]?.array ?? [] }
        func firstActivity(_ card: JSONObject) -> JSON? {
            let usage = bundle[card["id"]!.string!]?.object ?? JSONObject()
            return (card["counted"] != nil ? usage["counted"]?.object ?? JSONObject() : usage)["firstActivityAt"]
        }
        // Summary's chart draws what each source counts, so both series are rebucketed alike: the cards' own
        // histories and the counted parts.
        func rebucket(_ transform: ([JSON]) -> [JSON]) {
            for index in providers.indices {
                providers[index]["history"] = .array(transform(providers[index]["history"]!.array!))
                if var counted = providers[index]["counted"]?.object {
                    counted["history"] = .array(transform(counted["history"]!.array!))
                    providers[index]["counted"] = .object(counted)
                }
            }
        }
        var chartPositions: JSON = .null
        // Crop only today's chart, never the statistics window, keeping a short zero lead-in before the first call.
        if key == "today" && !slots.isEmpty {
            let included = providers.filter { $0["summaryIncluded"] == .bool(true) }
            let active = slots.indices.filter { i in included.contains { ((history($0)[i]).number ?? 0) > 0 } }
            if let firstIndex = active.first {
                let activity = included.compactMap { firstActivity($0) }.filter { !$0.isNull }.map { figure($0) }.filter { start <= $0 && $0 <= end }
                var firstCall = activity.min() ?? Swift.max(start, Double(slots[firstIndex].integer ?? 0))
                firstCall = Swift.min(firstCall, end - 0.001)
                let padding = Swift.min(300, Swift.max(30, (end - firstCall) * 0.06))
                let chartStart = Swift.max(start, firstCall - padding)
                // A midnight request still receives a tiny visual zero lead-in.
                var chartTimes = [chartStart, firstCall]
                var groups: [[Int]] = []
                for index in firstIndex..<slots.count {
                    let stamp = Swift.min(Double(slots[index].integer ?? 0) + 3600, end)
                    if chartTimes.count > 2 && stamp <= chartTimes[chartTimes.count - 1] { groups[groups.count - 1].append(index) }
                    else { chartTimes.append(stamp); groups.append([index]) }
                }
                let duration = Swift.max(0.001, end - chartStart)
                var positions = chartTimes.map { JSON.double(($0 - chartStart) / duration) }
                if (positions[1].number ?? 0) < 0.01 {
                    positions = [.int(0), .double(0.04)] + positions[2...].map { .double(0.04 + 0.96 * ($0.number ?? 0)) }
                }
                chartPositions = .array(positions)
                rebucket { series in [.int(0), .int(0)] + groups.map { group in group.reduce(JSON.int(0)) { Harness.add($0, series[$1]) } } }
                labels = chartTimes.map { Runtime.format($0, zone, "%H:%M") }
                span = Runtime.format(chartStart, zone, "%m/%d %H:%M") + "–" + Runtime.format(end, zone, "%H:%M")
            }
        }
        // Keep long histories legible without dropping recorded totals from the cards.
        var unit = short ? "每小时" : "每天"
        if slots.count > 60 {
            let size = (slots.count + 59) / 60
            let count = slots.count
            rebucket { series in stride(from: 0, to: count, by: size).map { start in series[start..<Swift.min(start + size, series.count)].reduce(JSON.int(0), Harness.add) } }
            labels = stride(from: 0, to: count, by: size).map { labels[$0] }
            unit = "每 \(size) 天"
        }
        var chartStatus: String
        if !bundle.isTruthy { chartStatus = Runtime.refreshFailed(rangeError) ? "数据暂不可用" : "待同步" }
        else { chartStatus = usageSources.contains { $0["error"]?.isTruthy == true || $0["chartUnavailable"]?.isTruthy == true } ? "部分图表数据不可用" : "" }
        if usageSources.contains(where: { $0["historyTruncated"]?.isTruthy == true || $0["entriesTruncated"]?.isTruthy == true }) { chartStatus = "仅含现有记录" }
        if key == "all" && usageSources.contains(where: { source in
            figure(source["summary"]?.object?["requests"]) > (source["days"]?.array ?? []).reduce(0) { $0 + figure($1["requests"]) }
        }) { chartStatus = "图表展示最近 366 天" }
        // The Summary adds every shown source, so its time is the oldest of their last good reads.
        let reads = usageSources.filter { $0["summary"]?.isTruthy == true }.map { Py.or($0["readAt"], bundle["_fetchedAt"] ?? .null) }
            .filter(\.isTruthy).compactMap(\.number)
        var updated = ""
        if let read = reads.min() {
            updated = "更新时间：" + Runtime.format(read, zone, Runtime.dayNumber(read, zone) == Runtime.dayNumber(now, zone) ? "%H:%M" : "%m/%d %H:%M")
        }
        let grouping = settings["groupBy"]?.string.flatMap { value in Runtime.groupings.contains { $0.0 == value } ? value : nil } ?? "tool"
        var cards: JSON = .null
        if bundle.isTruthy {
            if grouping == "model" { cards = .array(modelCards(selected, counting)) }
            else if grouping == "project" { cards = .array(projectCards(selected)) }
        }
        return [
            "date": .string(Runtime.iso(Runtime.dayNumber(now, zone))), "dates": .array(labels.map(JSON.string)), "providers": .array(providers.map(JSON.object)),
            "rangeKey": .string(key), "rangeLabel": .string(Runtime.ranges.first { $0.0 == key }!.1), "grouping": .string(grouping),
            "groupings": .array(Runtime.groupings.map { ["key": .string($0.0), "label": .string($0.1)] }), "cards": cards, "currency": currencyUnit(data),
            "groupNote": grouping == "project" && !cards.isNull ? "按客户端自己的记录" : .null,
            "chartUnit": .string(unit), "chartSpan": .string(span), "period": .array(period.map(JSON.string)), "chartStatus": .string(chartStatus),
            "updated": .string(updated), "chartPositions": chartPositions,
            "heatmap": moduleEnabled("heatmap") && !selected.isEmpty ? heatmapPayload(bundle, key: key, now: now, zone: zone, selected: selected, error: rangeError) : .null,
            "showSummary": .bool(moduleEnabled("summary")), "allowCollapse": settings["allowCollapse"] ?? .bool(false),
            "expandedProviders": settings["expandedProviders"] ?? .array([]),
        ]
    }

    /// The card's note for a module whose service has trouble right now, from its status page.
    static func serviceNote(_ statuses: JSON?, _ module: String) -> String? {
        let pages = ["claude": "claude", "claude-subscription": "claude", "claude-api": "claude", "codex": "openai", "opencodex": "openai",
                     "chatgpt": "openai", "chatgpt-api": "openai", "cursor": "cursor", "cursor-subscription": "cursor"]
        let indicators = ["none": "", "minor": "服务部分降级", "major": "服务大范围故障", "critical": "服务严重故障", "maintenance": "服务维护中"]
        guard let state = statuses?.object?[pages[module] ?? ""]?.object, state["error"]?.isTruthy != true else { return nil }
        return state["indicator"]?.string.flatMap { indicators[$0] }.flatMap { $0.isEmpty ? nil : $0 }
    }
}
