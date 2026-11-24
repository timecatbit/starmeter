"""Antigravity's own quota for Gemini 订阅, the one its app shows under Models & Usage: each group of models (Gemini's,
and Claude's with GPT's) has a five-hour and a weekly limit. The app runs a language server on this computer and notes
in ~/.gemini/antigravity/daemon/hub_<port>.json the process, the ports it listens on and the token it asks for; the
quota is asked of it there, on 127.0.0.1, and it answers from its own sign-in. Nothing else is sent anywhere, the
token is used only for that request and never kept, and with the app closed there is no one to ask. The engine's
antigravity.rs reads it the same way."""
from datetime import datetime
import json
import math
import os
from pathlib import Path
import urllib.request

# Said when no Antigravity is running to ask: the last figures stay, marked with when they were read.
NOT_RUNNING = 'Antigravity 没有运行'


def alive(pid):
    # 0 (to kill(), this process's own group) and ids past a positive pid_t, as a damaged note can hold, are none.
    if not isinstance(pid, int) or isinstance(pid, bool) or not 0 < pid <= 2**31 - 1:return False
    try:os.kill(pid, 0)
    except ProcessLookupError:return False
    except PermissionError:return True
    except OSError:return False
    return True


def hubs(home):
    """The language servers Antigravity's app noted, the newest first: (its process, its plain HTTP port, its token)."""
    folder = Path(home)/'.gemini'/'antigravity'/'daemon'
    try:entries = [path for path in folder.iterdir() if path.name.startswith('hub_') and path.name.endswith('.json')]
    except OSError:return []
    found = []
    for path in entries:
        try:changed = path.stat().st_mtime; note = json.loads(path.read_bytes())
        except (OSError, ValueError):continue
        if not isinstance(note, dict):continue
        pid, port, token = note.get('pid'), note.get('httpPort'), note.get('csrfToken')
        if not (isinstance(pid, int) and not isinstance(pid, bool) and 0 <= pid < 2**32):continue
        if not (isinstance(port, int) and not isinstance(port, bool) and 1 <= port < 65536) or not isinstance(token, str):continue
        found.append((changed, pid, port, token))
    found.sort(key=lambda hub: hub[0], reverse=True)
    return [(pid, port, token) for _, pid, port, token in found]


def present(home):
    """Whether Antigravity's app has run here and noted a language server, running now or not."""
    return bool(hubs(home))


def windows(summary):
    """The summary's buckets as windows: each group's five-hour and weekly limit, in the order the app lists them, a
    bucket without a share left out."""
    response = summary.get('response') if isinstance(summary, dict) else None
    groups = response.get('groups') if isinstance(response, dict) else None
    found = []
    for group in groups if isinstance(groups, list) else []:
        if not isinstance(group, dict):continue
        name = group.get('displayName') if isinstance(group.get('displayName'), str) else ''
        for bucket in group.get('buckets') if isinstance(group.get('buckets'), list) else []:
            if not isinstance(bucket, dict):continue
            ident = bucket.get('bucketId') if isinstance(bucket.get('bucketId'), str) else ''
            window = bucket.get('window') if isinstance(bucket.get('window'), str) else ident.rsplit('-', 1)[-1]
            left = bucket.get('remainingFraction')
            if not isinstance(left, (int, float)) or isinstance(left, bool) or window not in ('5h', 'weekly'):continue
            try:resets = datetime.fromisoformat(bucket['resetTime'].replace('Z', '+00:00')).timestamp()
            except (KeyError, AttributeError, ValueError):resets = None
            found.append({'group': ident.split('-')[0], 'groupName': name, 'window': window,
                          'remaining': math.floor(min(max(float(left), 0.0), 1.0) * 1000 + 0.5) / 10, 'resetsAt': resets})
    return found


def quota(home):
    """(the quota now as {'windows': […]}, None), Gemini's first; or (None, why): NOT_RUNNING when no noted server is
    alive, or why the one asked gave nothing."""
    failure = NOT_RUNNING
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    body = json.dumps({'metadata': {'ideName': 'antigravity', 'extensionName': 'antigravity', 'locale': 'en', 'ideVersion': 'unknown'}}).encode()
    for pid, port, token in hubs(home):
        if not alive(pid):continue
        request = urllib.request.Request('http://127.0.0.1:%d/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary' % port, data=body, method='POST',
                                         headers={'Content-Type': 'application/json', 'Connect-Protocol-Version': '1', 'X-Codeium-Csrf-Token': token})
        try:
            with opener.open(request, timeout=5) as answer:
                found = windows(json.loads(answer.read(1 << 20)))
        except urllib.error.HTTPError as error:
            failure = 'Antigravity 没有给出额度（HTTP %d）' % error.code;continue
        except ValueError:
            failure = 'Antigravity 没有给出额度';continue
        except OSError:
            # A note left by a server gone (its process id now another's) refuses the connection: what a live one
            # answered before it stays the reason, not 没有运行.
            continue
        if not found:
            failure = 'Antigravity 没有给出额度';continue
        return {'windows': found}, None
    return None, failure
