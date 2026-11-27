#!/usr/bin/env python3
"""Regenerate the price tables from a ccusage checkout (MIT, see lib/CCUSAGE_LICENSE).

lib/codex-pricing.json keeps the OpenAI models Codex logs can name, with ccusage's
Codex aliases, tiers and fallbacks. lib/model-pricing.json keeps the other vendors'
models that WorkBuddy, DeepSeek Harness, Gemini CLI and OpenCode may log. Prices are
USD per million tokens; missing cache prices use ccusage's defaults (read 10%,
write 125% of input). A daily GitHub Action runs it against the latest ccusage, and
installed plugins download the committed tables, so users get new prices without upgrading.

ccusage builds its snapshot from models.dev (MIT) and can fall days behind it. Given
models.dev's own api.json, OpenAI's first-party text models that models.dev updated after
the version ccusage's snapshot was built from, and that the snapshot lacks, are added as
models.dev lists them. Nothing ccusage prices is overridden, and once ccusage has a model
it comes from ccusage again.
"""
import argparse
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OPENAI = re.compile(r'(?:gpt-|o\d|codex-|chatgpt-)[^/@:]*')
# First-party names only; provider-prefixed resellers ("together-ai/…") price differently.
VENDORS = re.compile(r'(?:deepseek|glm|kimi|moonshot|minimax|qwen|gemini|grok|hunyuan|hy4)[^/@:~]*', re.I)

# Verified first-party vendor models not yet in ccusage's snapshot (USD per 1M tokens). Never overrides ccusage.
# Tencent Hunyuan 4 (TokenHub / Tencent AI official catalog, verified 2026-11-27):
#   hy4-flash: ¥0.50 / ¥2.00 / ¥0.02 (CNY) -> $0.070 / $0.281 / $0.003 (USD)
#   hy4:       $0.834 / $2.501 / $0.042 (USD)
SUPPLEMENT_OTHER_MODELS = {
    'hy4': {'input': 0.834, 'output': 2.501, 'cacheRead': 0.042, 'cacheWrite': 1.0425},
    'hy4-preview': {'input': 0.834, 'output': 2.501, 'cacheRead': 0.042, 'cacheWrite': 1.0425},
    'hy4-flash': {'input': 0.07, 'output': 0.281, 'cacheRead': 0.003, 'cacheWrite': 0.0875},
    'hy4flash': {'input': 0.07, 'output': 0.281, 'cacheRead': 0.003, 'cacheWrite': 0.0875},
}


def rates(cost):
    data = {'input': cost['input'], 'output': cost['output'],
            'cacheRead': cost.get('cache_read', cost['input'] * 0.1),
            'cacheWrite': cost.get('cache_write', cost['input'] * 1.25)}
    return {key: round(value, 6) for key, value in data.items()}


def table(snapshot, pattern, key=str):
    models = {}
    for name, entry in sorted(snapshot.items()):
        cost = entry.get('cost') or {}
        if not pattern.fullmatch(name) or not {'input', 'output'} <= set(cost):
            continue
        models[key(name)] = rates(cost)
        tier = next((t for t in cost.get('tiers', []) if t.get('tier', {}).get('type') == 'context'), None)
        if tier:
            models[key(name)]['longContext'] = dict(rates(tier), threshold=tier['tier']['size'])
    return models


def newer_openai(api, snapshot, built):
    """OpenAI's own text models that models.dev updated after the day ccusage's snapshot was built
    from and that the snapshot lacks, in the snapshot's shape."""
    found = {}
    for name, entry in sorted(((api.get('openai') or {}).get('models') or {}).items()):
        cost = entry.get('cost') if isinstance(entry, dict) else None
        if not OPENAI.fullmatch(name) or name in snapshot or not isinstance(cost, dict):continue
        if str(entry.get('last_updated') or '') <= built or 'text' not in ((entry.get('modalities') or {}).get('output') or []):continue
        prices = [cost.get(k) for k in ('input', 'output', 'cache_read', 'cache_write') if k in cost]
        if {'input', 'output'} <= set(cost) and all(isinstance(v, (int, float)) and not isinstance(v, bool) and v >= 0 for v in prices):
            found[name] = {'cost': cost}
    return found


def stamped(path, result, committed):
    """Installed plugins keep the copy with the latest committedAt. A table that did not change keeps
    its stamp; one that did takes ccusage's commit time, or a second past the last stamp when that is
    later (a models.dev addition while ccusage stands still), so a changed table always reads as newer."""
    try:current = json.loads(path.read_text())
    except (OSError, ValueError):current = {}
    before = (current.get('source') or {}).get('committedAt') if isinstance(current, dict) else None
    plain = lambda data:dict(data, source={k: v for k, v in data['source'].items() if k != 'committedAt'})
    if before and isinstance(current.get('source'), dict) and plain(current) == plain(dict(result, source=dict(result['source'], committedAt=before))):
        result['source']['committedAt'] = before;return result
    if before and datetime.fromisoformat(before) >= datetime.fromisoformat(committed):
        committed = (datetime.fromisoformat(before) + timedelta(seconds=1)).isoformat()
    result['source']['committedAt'] = committed
    return result


def model_aliases(repo):
    """The model names ccusage prices as another model's. Its pricing.rs had them in pricing_alias as
    `"gpt-5.6" => Some("gpt-5.6-sol")`; since November 2026 they are the ALIASES table in pricing/data.rs, as
    `("gpt-5.6", "gpt-5.6-sol")`. Neither found, the update stops and says so rather than writing a table without."""
    source = repo / 'rust/crates/ccusage-core/src'
    data = source / 'pricing/data.rs'
    if data.exists() and 'static ALIASES' in (text := data.read_text()):
        body = text.split('static ALIASES', 1)[1].split('\n];', 1)[0]
        return dict(re.findall(r'\(\s*"([^"]+)",\s*"([^"]+)"\s*\)', body))
    pricing = (source / 'pricing.rs').read_text() if (source / 'pricing.rs').exists() else ''
    if 'fn pricing_alias(' in pricing:
        body = pricing.split('fn pricing_alias(', 1)[1].split('\n}\n', 1)[0]
        return dict(re.findall(r'"([^"]+)" => Some\("([^"]+)"\)', body))
    raise SystemExit("ccusage's model aliases are in neither pricing/data.rs (ALIASES) nor pricing.rs (pricing_alias)")


def auto_review_fallbacks(repo):
    """Which model codex-auto-review stood for from which day, newest first. ccusage kept the timeline in
    codex-auto-review-fallbacks.json; since November 2026 it is CODEX_AUTO_REVIEW_FALLBACKS in parser/model.rs, as
    `CodexAutoReviewFallback::new((2026, 7, 30), "gpt-5.6-luna")`, its last entry naming the default model's constant."""
    codex = repo / 'rust/adapters/codex/src'
    listed = codex / 'codex-auto-review-fallbacks.json'
    if listed.exists():
        return json.loads(listed.read_text())
    model = codex / 'parser/model.rs'
    text = model.read_text() if model.exists() else ''
    default = re.search(r'CODEX_AUTO_REVIEW_DEFAULT_MODEL: &str = "([^"]+)"', text)
    entries = re.findall(r'CodexAutoReviewFallback::new\(\((\d+),\s*(\d+),\s*(\d+)\),\s*(?:"([^"]+)"|CODEX_AUTO_REVIEW_DEFAULT_MODEL)\)', text)
    if not entries or (default is None and any(not name for *_, name in entries)):
        raise SystemExit("ccusage's codex-auto-review timeline is in neither codex-auto-review-fallbacks.json nor parser/model.rs")
    return [{'releasedOn': '%04d-%02d-%02d' % (int(y), int(m), int(d)), 'model': name or default.group(1)} for y, m, d, name in entries]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('ccusage', type=Path, help='Path to a ccusage git checkout')
    parser.add_argument('--models-dev', type=Path, help="models.dev's api.json, for OpenAI models ccusage has not picked up yet")
    args = parser.parse_args()
    repo = args.ccusage.resolve()
    snapshot = json.loads((repo / 'rust/crates/ccusage-pricing-snapshots/src/models-dev-pricing.json').read_text())
    added = {}
    if args.models_dev:
        # The models.dev version ccusage's snapshot was built from, as pinned in its flake.lock.
        locked = json.loads((repo / 'flake.lock').read_text())['nodes']['models-dev']['locked']['lastModified']
        built = datetime.fromtimestamp(locked, timezone.utc).date().isoformat()
        added = newer_openai(json.loads(args.models_dev.read_text()), snapshot, built)
    models = table(dict(snapshot, **added), OPENAI)
    aliases = model_aliases(repo)
    multipliers = {}
    for tier in ('fast', 'flex', 'ultrafast'):
        exact = json.loads((repo / 'rust/crates/ccusage-core/src' / (tier + '-multiplier-overrides.json')).read_text())['exact']
        multipliers[tier] = {name: value for name, value in sorted(exact.items()) if name in models}
    fallbacks = auto_review_fallbacks(repo)
    commit = subprocess.run(['git', '-C', str(repo), 'rev-parse', 'HEAD'], capture_output=True, text=True, check=True).stdout.strip()
    # The commit time orders tables: installed plugins keep whichever copy is newest.
    committed = subprocess.run(['git', '-C', str(repo), 'log', '-1', '--format=%cI'], capture_output=True, text=True, check=True).stdout.strip()
    source = {'project': 'ccusage', 'commit': commit, 'committedAt': committed, 'license': 'MIT'}
    codex_source = dict(source, supplement={'project': 'models.dev', 'license': 'MIT', 'models': sorted(added)}) if added else dict(source)
    result = {'source': codex_source, 'aliases': aliases, 'autoReviewFallbacks': fallbacks, 'multipliers': multipliers, 'models': models}
    path = ROOT / 'lib/codex-pricing.json'
    path.write_text(json.dumps(stamped(path, result, committed), ensure_ascii=False, indent=1) + '\n')
    # Lower-case keys: harness logs spell vendor model names inconsistently.
    others = table(snapshot, VENDORS, key=str.lower)
    for name, entry in SUPPLEMENT_OTHER_MODELS.items():
        others.setdefault(name, entry)
    path = ROOT / 'lib/model-pricing.json'
    path.write_text(json.dumps(stamped(path, {'source': dict(source), 'models': others}, committed), ensure_ascii=False, indent=1) + '\n')
    print('codex models:', len(models), 'other models:', len(others), 'commit:', commit[:8], 'from models.dev:', ', '.join(sorted(added)) or 'none')


if __name__ == '__main__':
    main()
