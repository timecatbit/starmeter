"""Claude Code and Codex sessions that are running or waiting for input now.

Only the end of each transcript changed in the last half hour is read. Of a session, nothing is kept but
its state, the title its client gave it (named by the user or by the client), the folder name of its project,
its model and reasoning effort, how much of its context window the last request filled, and when it last
changed; never prompts, replies or paths. A session is identified by a hash.
"""
import hashlib
import json
import os
import re
from urllib.parse import unquote
from pathlib import Path

ACTIVE = 1800   # A session waiting for input stays listed this long after its last change.
STALLED = 600   # A running session silent this long has most likely stopped without saying so.
TAIL = 512 * 1024
# Claude's context window: 200K, unless a request already went past it (the 1M window).
CLAUDE_WINDOWS = (200_000, 1_000_000)


def project_name(path):
    """The folder's name; none for the folders the ChatGPT app makes for a chat or a project, named by date and ID."""
    if not isinstance(path, str) or not path.strip():return None
    path = path.strip()
    if path.startswith('file://'):path = unquote(path[7:])
    path = path.rstrip('/\\').replace('\\', '/')
    if re.search(r'/Codex/\d{4}-\d{2}-\d{2}/[^/]+$|/\.chatgpt-projects/g-p-[0-9a-f]+$', path):return None
    return path.rsplit('/', 1)[-1] or None


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


def text(value):
    return value.strip() if isinstance(value, str) and value.strip() else None


def claude(path):
    """A Claude Code transcript: waiting once the last reply ended its turn, running otherwise. Its title is the
    last one the transcript records: the name the user or the App gave it, else the one Claude Code made up."""
    last = reply = None; folder = None; named = made = None
    for row in rows_at_end(path):
        if row.get('type') == 'custom-title':named = text(row.get('customTitle')) or named
        elif row.get('type') == 'ai-title':made = text(row.get('aiTitle')) or made
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
    return {'tool': 'Claude', 'state': 'waiting' if waiting else 'running', 'title': named or made, 'project': project_name(folder),
            'model': model, 'effort': None, 'context': context}


def codex(path):
    """A Codex rollout: running between task_started and task_complete (or an aborted turn)."""
    head = first_row(path); payload = head.get('payload') if isinstance(head.get('payload'), dict) else {}
    # A subagent's thread (Guardian reviews among them) works for another session, like Claude's sidechains.
    if isinstance(payload.get('source'), dict) and 'subagent' in payload['source']:return None
    folder = payload.get('cwd') if head.get('type') == 'session_meta' else None
    state = None; model = None; context = None; effort = None
    for row in rows_at_end(path):
        data = row.get('payload') if isinstance(row.get('payload'), dict) else {}
        kind = data.get('type')
        if row.get('type') == 'turn_context':model = data.get('model') or model;effort = text(data.get('effort')) or effort
        if row.get('type') == 'session_meta' and data.get('cwd'):folder = data['cwd']
        if kind == 'task_started':state = 'running'
        elif kind in ('task_complete', 'turn_aborted'):state = 'waiting'
        elif kind == 'token_count' and isinstance(data.get('info'), dict):
            info = data['info']; last = info.get('last_token_usage') or {}; window = number(info.get('model_context_window'))
            used = number(last.get('input_tokens')) + number(last.get('output_tokens'))
            if window and used:context = min(1.0, used / window)
    if state is None:return None
    return {'tool': 'ChatGPT', 'state': state, 'title': None, 'project': project_name(folder), 'model': model, 'effort': effort,
            'context': context}


def codex_titles(codex_root):
    """Codex's thread names by thread ID, from its session index; the last name given wins."""
    names = {}
    try:
        with open(Path(codex_root) / 'session_index.jsonl', 'rb') as handle:
            for line in handle:
                try:row = json.loads(line)
                except ValueError:continue
                if isinstance(row, dict) and isinstance(row.get('id'), str) and text(row.get('thread_name')):
                    names[row['id']] = text(row['thread_name'])
    except OSError:pass
    return names


# Session detail reads further back than the state does: the last few turns of a long session.
DETAIL = 2 * 1024 * 1024
TURNS = 8


def stamp(value):
    """Seconds since the epoch from a transcript's ISO time, or None."""
    from datetime import datetime
    try:return datetime.fromisoformat(str(value).replace('Z', '+00:00')).timestamp()
    except ValueError:return None


def turn(at):
    return {'at': at, 'requests': 0, 'tokens': 0, 'cost': 0.0, 'unpriced': 0, 'tools': {}}


def claude_turns(path, price):
    """The last turns of a Claude Code session, newest first: from one prompt to the next, its requests, tokens,
    estimated cost and the tools it called. A reply streamed over several lines counts once; a subagent's work
    (sidechain rows) belongs to the turn that started it. price(model, usage) gives a request's cost or None."""
    # A turn that began before the part read is counted from where reading starts, and marked so.
    turns = [turn(None)]; current = turns[0]; seen = set()
    for row in rows_at_end(path, DETAIL):
        message = row.get('message') if isinstance(row.get('message'), dict) else {}
        content = message.get('content')
        if row.get('type') == 'user' and not row.get('isSidechain') and not row.get('isMeta'):
            kinds = {item.get('type') for item in content if isinstance(item, dict)} if isinstance(content, list) else set()
            if (isinstance(content, str) and content.strip()) or (kinds and 'tool_result' not in kinds):
                current = turn(stamp(row.get('timestamp'))); turns.append(current)
        elif row.get('type') == 'assistant' and current is not None:
            for item in content if isinstance(content, list) else []:
                if isinstance(item, dict) and item.get('type') == 'tool_use' and item.get('name'):
                    current['tools'][item['name']] = current['tools'].get(item['name'], 0) + 1
            usage = message.get('usage'); key = message.get('id') or row.get('uuid')
            if isinstance(usage, dict) and key not in seen:
                seen.add(key); current['requests'] += 1
                current['tokens'] += sum(number(usage.get(k)) for k in ('input_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens', 'output_tokens'))
                cost = price(message.get('model'), usage)
                if cost is None:current['unpriced'] += 1
                else:current['cost'] += cost
    return [t for t in turns if t['at'] is not None or t['requests']][-TURNS:][::-1]


def codex_turns(path, price):
    """The last turns of a Codex session, newest first: from task_started to task_complete, its requests (each
    change of the running token total), tokens, estimated cost and the tools it called. price(model, usage)."""
    turns = [turn(None)]; current = turns[0]; model = None; total = None
    for row in rows_at_end(path, DETAIL):
        data = row.get('payload') if isinstance(row.get('payload'), dict) else {}
        kind = data.get('type')
        if row.get('type') == 'turn_context':model = data.get('model') or model
        if kind == 'task_started':
            current = turn(stamp(row.get('timestamp'))); turns.append(current)
        elif row.get('type') == 'response_item' and kind in ('function_call', 'custom_tool_call', 'local_shell_call', 'web_search_call'):
            name = data.get('name') or {'local_shell_call': 'shell', 'web_search_call': 'web_search'}.get(kind)
            if name:current['tools'][name] = current['tools'].get(name, 0) + 1
        elif kind == 'token_count' and isinstance(data.get('info'), dict):
            info = data['info']; running = info.get('total_token_usage'); last = info.get('last_token_usage') or {}
            if not last or running == total:continue
            total = running; current['requests'] += 1
            current['tokens'] += number(last.get('input_tokens')) + number(last.get('output_tokens'))
            cost = price(model, last)
            if cost is None:current['unpriced'] += 1
            else:current['cost'] += cost
    return [t for t in turns if t['at'] is not None or t['requests']][-TURNS:][::-1]


def detail(session_path, tool, prices):
    """A listed session's last turns; prices maps the tool to its price function."""
    read = claude_turns if tool == 'Claude' else codex_turns
    try:return read(session_path, prices[tool])
    except (OSError, ValueError, TypeError, AttributeError, KeyError):return []


def recent(root, pattern, since):
    try:return [p for p in Path(root).glob(pattern) if p.is_file() and p.stat().st_mtime >= since]
    except OSError:return []


def active(claude_roots, codex_root, now, prices=None, claude_app=None):
    """Sessions changed in the last half hour, the most recent first. A running one silent for ten minutes is dropped.
    With prices (tool -> price(model, usage)), each carries its last turns as well. claude_app maps a transcript's
    session ID to the Claude App's title and effort for it."""
    found = []; titles = None
    candidates = [(claude, p) for root in claude_roots for p in recent(root, '*/*.jsonl', now - ACTIVE) if 'subagents' not in p.parts]
    if codex_root:
        # A rollout is filed under the day its thread started, and a thread can still be going days later.
        candidates += [(codex, p) for p in recent(Path(codex_root) / 'sessions', '*/*/*/rollout-*.jsonl', now - ACTIVE)]
    for read, path in candidates:
        try:
            changed = path.stat().st_mtime; session = read(path)
        except (OSError, ValueError, TypeError, AttributeError):continue
        if not session or (session['state'] == 'running' and now - changed > STALLED):continue
        if session['tool'] == 'Claude':
            app = (claude_app or {}).get(path.stem) or {}
            session['title'] = session['title'] or app.get('title'); session['effort'] = app.get('effort')
        else:
            # rollout-<started>-<thread ID>.jsonl
            titles = codex_titles(codex_root) if titles is None else titles
            session['title'] = titles.get(path.stem[-36:])
        entry = dict(session, id=hashlib.sha256(str(path).encode()).hexdigest()[:16], at=changed)
        # The turns are read here, so the path itself never leaves this function.
        if prices:entry['turns'] = detail(path, session['tool'], prices)
        found.append(entry)
    return sorted(found, key=lambda s: -s['at'])


def newly_waiting(previous, current, now, recent=180):
    """Sessions that were running at the last read and have just finished their turn."""
    before = {s['id']: s['state'] for s in previous or [] if isinstance(s, dict) and 'id' in s}
    return [s for s in current if s['state'] == 'waiting' and before.get(s['id']) == 'running' and now - s['at'] <= recent]
