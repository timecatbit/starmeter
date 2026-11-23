"""Official service status pages, read only when the user turns 服务状态 on.

Each page is a public Statuspage-style document (`/api/v2/status.json`); nothing about the user is sent,
no cookies, only a plain User-Agent. A page that can't be read says so for itself and never hides the others.
"""
import json
import urllib.request

# page id -> (name, status document, page for people)
PAGES = {
    'claude': ('Claude', 'https://status.claude.com/api/v2/status.json', 'https://status.claude.com'),
    'openai': ('OpenAI', 'https://status.openai.com/api/v2/status.json', 'https://status.openai.com'),
    'cursor': ('Cursor', 'https://status.cursor.com/api/v2/status.json', 'https://status.cursor.com'),
}
# Which page speaks for each module.
MODULE_PAGES = {'claude': 'claude', 'claude-subscription': 'claude', 'claude-api': 'claude',
                'codex': 'openai', 'opencodex': 'openai', 'chatgpt': 'openai', 'chatgpt-api': 'openai',
                'cursor': 'cursor', 'cursor-subscription': 'cursor'}
# What a page's indicator means, in the panel's words; 'none' says nothing.
INDICATORS = {'none': '', 'minor': '服务部分降级', 'major': '服务大范围故障', 'critical': '服务严重故障', 'maintenance': '服务维护中'}


def read(page, version, opener=urllib.request.urlopen):
    """One page's indicator and description, or an error."""
    _, url, _ = PAGES[page]
    request = urllib.request.Request(url, headers={'User-Agent': 'Starmeter/' + version, 'Accept': 'application/json'})
    try:
        with opener(request, timeout=10) as response:body = response.read(64 * 1024 + 1)
        if len(body) > 64 * 1024:raise ValueError('too large')
        status = json.loads(body).get('status') or {}
        indicator = status.get('indicator'); description = status.get('description')
        if indicator not in INDICATORS or not isinstance(description, str):raise ValueError('unexpected status')
        return {'indicator': indicator, 'description': description[:200]}
    except (OSError, ValueError, AttributeError) as exc:
        return {'error': type(exc).__name__}


def read_all(pages, version, opener=urllib.request.urlopen):
    return {page: read(page, version, opener) for page in pages if page in PAGES}


def merged(previous, fresh):
    """A new reading over the last one: a page that failed this time (offline, just woken) keeps the last good answer
    it had, marked 'kept'; one never read well shows its failure (WAKE-2)."""
    out = {}
    for key, value in fresh.items():
        last = (previous or {}).get(key)
        if isinstance(value, dict) and value.get('error') and isinstance(last, dict) and 'indicator' in last:
            out[key] = dict(last, kept=True)
        else:
            out[key] = value
    return out


def unsettled(payload):
    """Whether a page's answer is a failure or a kept one: read again in a minute, not five."""
    return any(isinstance(value, dict) and (value.get('error') or value.get('kept')) for value in (payload or {}).values())


def lifetime(payload):
    """How long a reading of the pages holds."""
    return 60 if unsettled(payload) else 300


def note(statuses, module):
    """The card's note for a module whose service has trouble right now, or None."""
    state = (statuses or {}).get(MODULE_PAGES.get(module, ''))
    if not isinstance(state, dict) or state.get('error'):return None
    return INDICATORS.get(state.get('indicator')) or None
