"""Usage export: the figures of the range on screen, for spreadsheets and scripts.

CSV has one row per day and tool; JSON holds each tool's totals, models and days. Both also give what
Summary counts for each tool, so adding them up does not count a request a gateway also logged twice.
Only counters leave: no paths, keys, prompts or session IDs.
"""
import csv
from datetime import datetime, timezone
import io
import json

COLUMNS = ('date', 'tool', 'requests', 'input_tokens', 'output_tokens', 'cache_read_tokens', 'cache_write_tokens',
           'total_tokens', 'cost_usd', 'unpriced_requests', 'credits', 'summary_tokens', 'summary_cost_usd')


def number(value):
    try:
        value = float(value)
        return value if value >= 0 else 0.0
    except (TypeError, ValueError):
        return 0.0


def counters(row):
    """One tally row (summary, model, day) in the export's own names. Inputs include cache reads and writes."""
    writes = row.get('cacheCreationInputTokens')
    return {'requests': int(number(row.get('requests'))), 'input_tokens': int(number(row.get('inputTokens'))),
            'output_tokens': int(number(row.get('outputTokens'))),
            'cache_read_tokens': int(number(row.get('cacheReadInputTokens') or row.get('cachedInputTokens'))),
            'cache_write_tokens': int(number(writes)) if writes is not None else None,
            'total_tokens': int(number(row.get('totalTokens'))), 'cost_usd': round(number(row.get('estimatedCostUsd')), 6),
            'unpriced_requests': int(number(row.get('unpricedRequests'))), 'credits': round(number(row.get('credits')), 4)}


def tools(selected, counting):
    """(id, name, its figures, what Summary counts of it) for every tool that has figures."""
    out = []
    for key, name, usage in selected:
        if not usage.get('summary') or usage.get('error'):continue
        out.append((key, name, usage, counting.get(key, {}).get('usage') or {}))
    return out


def csv_text(selected, counting):
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator='\n')
    writer.writerow(COLUMNS)
    for _, name, usage, counted in tools(selected, counting):
        counted_days = {str(day.get('date', ''))[:10]: day for day in counted.get('days') or []}
        for day in sorted(usage.get('days') or [], key=lambda d: str(d.get('date', ''))):
            date = str(day.get('date', ''))[:10]; part = counted_days.get(date) or {}
            row = dict(counters(day), date=date, tool=name, summary_tokens=int(number(part.get('totalTokens'))),
                       summary_cost_usd=round(number(part.get('estimatedCostUsd')), 6))
            writer.writerow(['' if row[column] is None else row[column] for column in COLUMNS])
    return buffer.getvalue()


def json_text(selected, counting, key, label, start, end, version):
    def moment(stamp):
        return datetime.fromtimestamp(stamp, timezone.utc).isoformat() if stamp else None
    body = {'product': 'Starmeter', 'version': version, 'range': key, 'rangeLabel': label, 'start': moment(start), 'end': moment(end),
            'exportedAt': datetime.now(timezone.utc).isoformat(), 'tools': []}
    for tool, name, usage, counted in tools(selected, counting):
        body['tools'].append({'id': tool, 'name': name, 'totals': counters(usage['summary']),
                              'summary': counters(counted.get('summary') or {}),
                              'models': [dict(counters(row), model=str(row.get('model') or '')) for row in usage.get('models') or []],
                              'days': [dict(counters(row), date=str(row.get('date', ''))[:10]) for row in usage.get('days') or []]})
    return json.dumps(body, ensure_ascii=False, indent=2) + '\n'
