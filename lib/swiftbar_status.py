"""Official service status pages, read only when the user turns 服务状态 on.

Each page is a public Statuspage-style summary (`/api/v2/summary.json`: its status, the parts not working and the
incidents open now); nothing about the user is sent, no cookies, only a plain User-Agent. A page that can't be read
says so for itself and never hides the others.
"""
import json
import urllib.request

# page id -> (name, status document, page for people)
PAGES = {
    'claude': ('Claude', 'https://status.claude.com/api/v2/summary.json', 'https://status.claude.com'),
    'openai': ('OpenAI', 'https://status.openai.com/api/v2/summary.json', 'https://status.openai.com'),
    'cursor': ('Cursor', 'https://status.cursor.com/api/v2/summary.json', 'https://status.cursor.com'),
}
# Which page speaks for each module.
MODULE_PAGES = {'claude': 'claude', 'claude-subscription': 'claude', 'claude-api': 'claude',
                'codex': 'openai', 'opencodex': 'openai', 'chatgpt': 'openai', 'chatgpt-api': 'openai',
                'cursor': 'cursor', 'cursor-subscription': 'cursor'}
# What a page's indicator means, in the panel's words; 'none' says nothing.
INDICATORS = {'none': '', 'minor': '服务部分降级', 'major': '服务大范围故障', 'critical': '服务严重故障', 'maintenance': '服务维护中'}
# What an open incident or maintenance is doing; one not listed (resolved) isn't shown.
INCIDENT_STATES = {'investigating': '正在调查', 'identified': '已找到原因', 'monitoring': '已修复，观察中',
                   'in_progress': '维护中', 'verifying': '维护验证中', 'scheduled': '计划维护'}
# A part of the service that isn't working fully.
COMPONENT_STATES = {'degraded_performance': '性能下降', 'partial_outage': '部分中断', 'major_outage': '大范围中断', 'under_maintenance': '维护中'}


def read(page, version, opener=urllib.request.urlopen):
    """One page's indicator and description, with the incidents and maintenances open now and the parts not working
    fully, so a note of trouble says what it is and where to read more; or an error."""
    _, url, _ = PAGES[page]
    request = urllib.request.Request(url, headers={'User-Agent': 'Starmeter/' + version, 'Accept': 'application/json'})
    try:
        with opener(request, timeout=10) as response:body = response.read(256 * 1024 + 1)
        if len(body) > 256 * 1024:raise ValueError('too large')
        return summary(json.loads(body))
    except (OSError, ValueError, AttributeError) as exc:
        return {'error': type(exc).__name__}


def summary(document):
    """A summary document as kept: the status, then up to three open incidents or maintenances and five parts."""
    status = document.get('status') or {}
    indicator = status.get('indicator'); description = status.get('description')
    if indicator not in INDICATORS or not isinstance(description, str):raise ValueError('unexpected status')
    result = {'indicator': indicator, 'description': description.strip()[:200]}
    listed = lambda field: [x for x in (document.get(field) if isinstance(document.get(field), list) else []) if isinstance(x, dict)]
    incidents = []
    for incident in listed('incidents') + listed('scheduled_maintenances'):
        state = incident.get('status'); name = incident.get('name')
        if not isinstance(state, str) or state not in INCIDENT_STATES or state == 'scheduled' or not isinstance(name, str):continue
        link = incident.get('shortlink')
        incidents.append({'name': name.strip()[:120], 'status': state,
                          'link': link.strip()[:300] if isinstance(link, str) and link.startswith('https://') else None})
    if incidents:result['incidents'] = incidents[:3]
    parts = [{'name': part['name'].strip()[:60], 'status': part['status']} for part in listed('components')
             if part.get('group') is not True and isinstance(part.get('status'), str) and part['status'] in COMPONENT_STATES
             and isinstance(part.get('name'), str)]
    if parts:result['components'] = parts[:5]
    return result


def trouble(state):
    """What is wrong, a line each: the open incidents with what is being done, or else the parts not working fully;
    None when the page names neither."""
    def named(field, states):
        return ['%s · %s' % (item.get('name'), states[item['status']]) for item in (state.get(field) or [])
                if isinstance(item, dict) and item.get('status') in states]
    lines = named('incidents', INCIDENT_STATES) or named('components', COMPONENT_STATES)
    return '\n'.join(lines) if lines else None



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
