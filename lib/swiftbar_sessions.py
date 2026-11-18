"""Claude Code and Codex sessions that are running or waiting for input now.

Only the end of each transcript changed in the last half hour is read. Of a session, nothing is kept but
its state, the folder name of its project, its model, how much of its context window the last request
filled, and when it last changed; never prompts, replies or paths. A session is identified by a hash.
"""
import hashlib
import json
import os
from pathlib import Path
import time

ACTIVE = 1800   # A session waiting for input stays listed this long after its last change.
STALLED = 600   # A running session silent this long has most likely stopped without saying so.
TAIL = 512 * 1024
# Claude's context window: 200K, unless a request already went past it (the 1M window).
CLAUDE_WINDOWS = (200_000, 1_000_000)


def project_name(path):
    if not isinstance(path, str) or not path.strip():return None
    return path.strip().rstrip('/\\').replace('\\', '/').rsplit('/', 1)[-1] or None


def rows_at_end(path, size=TAIL):
    """The complete JSON lines in the last `size` bytes of a file, in order."""
    with open(path, 'rb') as handle:
        handle.seek(0, os.SEEK_END); end = handle.tell(); handle.seek(max(0, end - size)); data = handle.read()
    lines = data.split(b'\n')
    if end > size:lines = lines[1:]  # The first line was cut.
    rows = []
    for line in lines:
        try:
            row = json.loads(line)
            if isinstance(row, dict):rows.append(row)
        except ValueError:continue
    return rows


def first_row(path):
    with open(path, 'rb') as handle:
        try:return json.loads(handle.readline(TAIL))
        except ValueError:return {}


def number(value):
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0 else 0


def claude(path):
    """A Claude Code transcript: waiting once the last reply ended its turn, running otherwise."""
    last = reply = None; folder = None
    for row in rows_at_end(path):
        if row.get('isSidechain'):continue
        if row.get('cwd'):folder = row['cwd']
        if row.get('type') in ('assistant', 'user'):last = row
        message = row.get('message') if isinstance(row.get('message'), dict) else {}
        if row.get('type') == 'assistant' and isinstance(message.get('usage'), dict):reply = message
    if last is None:return None
    message = last.get('message') if isinstance(last.get('message'), dict) else {}
    waiting = last.get('type') == 'assistant' and message.get('stop_reason') in ('end_turn', 'stop_sequence', 'max_tokens', 'refusal')
    context = None; model = None
    if reply:
        usage = reply['usage']; model = reply.get('model')
        used = sum(number(usage.get(k)) for k in ('input_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens', 'output_tokens'))
        window = next((w for w in CLAUDE_WINDOWS if used <= w), CLAUDE_WINDOWS[-1])
        context = min(1.0, used / window) if used else None
    return {'tool': 'Claude', 'state': 'waiting' if waiting else 'running', 'project': project_name(folder), 'model': model, 'context': context}


def codex(path):
    """A Codex rollout: running between task_started and task_complete (or an aborted turn)."""
    head = first_row(path); payload = head.get('payload') if isinstance(head.get('payload'), dict) else {}
    folder = payload.get('cwd') if head.get('type') == 'session_meta' else None
    state = None; model = None; context = None
    for row in rows_at_end(path):
        data = row.get('payload') if isinstance(row.get('payload'), dict) else {}
        kind = data.get('type')
        if row.get('type') == 'turn_context':model = data.get('model') or model
        if row.get('type') == 'session_meta' and data.get('cwd'):folder = data['cwd']
        if kind == 'task_started':state = 'running'
        elif kind in ('task_complete', 'turn_aborted'):state = 'waiting'
        elif kind == 'token_count' and isinstance(data.get('info'), dict):
            info = data['info']; last = info.get('last_token_usage') or {}; window = number(info.get('model_context_window'))
            used = number(last.get('input_tokens')) + number(last.get('output_tokens'))
            if window and used:context = min(1.0, used / window)
    if state is None:return None
    return {'tool': 'ChatGPT', 'state': state, 'project': project_name(folder), 'model': model, 'context': context}


def recent(root, pattern, since):
    try:return [p for p in Path(root).glob(pattern) if p.is_file() and p.stat().st_mtime >= since]
    except OSError:return []


def active(claude_roots, codex_root, now):
    """Sessions changed in the last half hour, the most recent first. A running one silent for ten minutes is dropped."""
    found = []
    candidates = [(claude, p) for root in claude_roots for p in recent(root, '*/*.jsonl', now - ACTIVE) if 'subagents' not in p.parts]
    if codex_root:
        # Rollouts are filed by the day they started: today's and yesterday's folders hold every session still going.
        days = {time.strftime('%Y/%m/%d', time.localtime(now - shift)) for shift in (0, 86400)}
        candidates += [(codex, p) for day in sorted(days) for p in recent(Path(codex_root) / 'sessions' / day, 'rollout-*.jsonl', now - ACTIVE)]
    for read, path in candidates:
        try:
            changed = path.stat().st_mtime; session = read(path)
        except (OSError, ValueError, TypeError, AttributeError):continue
        if not session or (session['state'] == 'running' and now - changed > STALLED):continue
        found.append(dict(session, id=hashlib.sha256(str(path).encode()).hexdigest()[:16], at=changed))
    return sorted(found, key=lambda s: -s['at'])


def newly_waiting(previous, current, now, recent=180):
    """Sessions that were running at the last read and have just finished their turn."""
    before = {s['id']: s['state'] for s in previous or [] if isinstance(s, dict) and 'id' in s}
    return [s for s in current if s['state'] == 'waiting' and before.get(s['id']) == 'running' and now - s['at'] <= recent]
