"""First-install module discovery. Never parse conversation bodies or make requests."""
import json
import os
from pathlib import Path
import shutil
import swiftbar_accounts as accounts
import swiftbar_catalog as catalog
import swiftbar_preferences as preferences
import swiftbar_claude as claude_desktop
import swiftbar_qoder as qoder
import swiftbar_antigravity as antigravity_quota
from swiftbar_sources import antigravity_stores, dsh_sessions_root, magpie_root
from swiftbar_config import monitor_url


def read_object(path):
    try:
        value=json.loads(Path(path).read_text(encoding='utf-8'))
        return value if isinstance(value,dict) else {}
    except (OSError,ValueError):return {}


def has_records(root, patterns=('*.jsonl',)):
    if not root:return False
    root=Path(root).expanduser()
    if not root.is_dir():return False
    try:return any(path.is_file() and path.stat().st_size>0 for pattern in patterns for path in root.rglob(pattern))
    except OSError:return False


# Where the ChatGPT app keeps Codex's command line, in an apps folder: the app (com.openai.codex) carries it.
CHATGPT_APP_CODEX='ChatGPT.app/Contents/Resources/codex-cli/bin/codex'


def chatgpt_codex(which=None, app_roots=()):
    """Codex's own command line on PATH, else the one inside the ChatGPT app, which signs in to the same ~/.codex: the
    app alone, without Codex installed, has the plan's windows."""
    return (which or shutil.which)('codex') or next((str(Path(root)/CHATGPT_APP_CODEX) for root in app_roots if (Path(root)/CHATGPT_APP_CODEX).is_file()),None)


def detect(paths, home=None, configured_proxy=False, komari_url='', which=shutil.which, app_roots=(), chatgpt_home=None, configured_subscriptions=()):
    home=Path(home or Path.home());found={'summary','heatmap','updates'}
    if configured_proxy:found.update(('opencodex','chatgpt'))
    for source,binary in (('codex','codex'),('opencode','opencode'),('gemini','gemini'),('deepseek-harness','dsh')):
        if which(binary):found.add(source)
    magpie_log=Path(paths.get('magpie') or magpie_root(home))/'usage.jsonl'
    if which('magpie') or magpie_log.is_file() or any((Path(root)/name).is_dir() for root in app_roots for name in ('magpie.app','Magpie.app')):
        found.add('magpie')
    if any((Path(root)/'WorkBuddy.app').is_dir() for root in app_roots):found.add('workbuddy')
    # Cursor keeps no usage on disk; the module only offers to read the account's records once turned on.
    if which('cursor-agent') or any((Path(root)/'Cursor.app').is_dir() for root in app_roots) or (Path(paths.get('cursor') or home)/'state.vscdb').is_file():found.update(('cursor','cursor-subscription'))
    root=Path(paths.get('qoder') or os.environ.get('QODERCN_CONFIG_DIR') or home/'.qoder-cn')
    if which('qodercn') or any((Path(r)/name).is_dir() for r in app_roots for name in ('Qoder CN.app','QoderCN.app')) or has_records(root/'projects'):found.add('qoder')
    if qoder.snapshot(home):found.add('qoder-subscription')
    claude_root=Path(paths.get('claude') or claude_desktop.default_root(home))
    if (any((Path(root)/'Claude.app').is_dir() for root in app_roots) or claude_desktop.registry_files(claude_root) or which('claude')
            or any(has_records(folder) for folder in claude_desktop.project_roots(home))):found.add('claude')
    if claude_desktop.subscription(claude_root).get('windows'):found.add('claude-subscription')
    for source in ('codex',):
        root=Path(paths.get(source) or home/'.codex')
        folders=[root]
        if source=='codex':folders=[root/'sessions',root/'archived_sessions'] if any((root/k).is_dir() for k in ('sessions','archived_sessions')) else [root]
        if any(has_records(folder,('*.jsonl','*.jsonl.zst')) for folder in folders):found.add(source)
    # Active sessions are Claude Code's and Codex's.
    if found & {'claude','codex'}:found.add('sessions')
    root=Path(paths.get('opencode') or home/'.local/share/opencode')
    try:
        database=root/'opencode.db' if root.is_dir() else root
        if database.is_file() and database.stat().st_size>0:found.add('opencode')
    except OSError:pass
    if has_records(paths.get('deepseek-harness') or dsh_sessions_root(home),('session*.jsonl','session*.jsonl.zstd')):found.add('deepseek-harness')
    root=Path(paths.get('gemini') or home/'.gemini')
    # Antigravity is one of Gemini's clients, like Gemini CLI: its app, its agy CLI or a conversation it keeps.
    antigravity=any((Path(r)/'Antigravity.app').is_dir() for r in app_roots) or which('agy') or antigravity_stores(root)
    # Gemini 订阅 comes from Antigravity's quota: its app (or the Gemini app beside it) installed, or a note it left.
    if any((Path(r)/name).is_dir() for r in app_roots for name in ('Antigravity.app','Gemini.app')) or antigravity_quota.present(home):found.add('gemini-subscription')
    if antigravity or has_records(root/'tmp' if (root/'tmp').is_dir() else root,('session-*.json','session-*.jsonl','chats/**/*.jsonl')):found.add('gemini')
    root=Path(paths.get('workbuddy') or home)
    roots=[root/n/'projects' for n in ('.workbuddy-ai','.workbuddy') if (root/n/'projects').is_dir()]
    if not roots and root!=home:roots=[root/'projects' if (root/'projects').is_dir() else root]
    if any(has_records(folder) for folder in roots):found.add('workbuddy')
    auth_root=Path(chatgpt_home or os.environ.get('CODEX_HOME') or home/'.codex').expanduser()
    auth=read_object(auth_root/'auth.json')
    if chatgpt_codex(which,app_roots) and (auth.get('auth_mode') in ('chatgpt','chatgptAuthTokens') or isinstance(auth.get('tokens'),dict) and auth['tokens'].get('access_token')):
        found.add('chatgpt')
    config=read_object(Path(paths.get('opencodex') or home/'.opencodex')/'config.json')
    providers=config.get('providers') or {}
    provider=providers.get('deepseek') if isinstance(providers,dict) else {}
    provider=provider if isinstance(provider,dict) else {}
    official=str(provider.get('baseUrl') or '').rstrip('/') in ('https://api.deepseek.com','https://api.deepseek.com/v1')
    if accounts.credential('DEEPSEEK_API_KEY') or official and provider.get('apiKey'):found.add('deepseek')
    if accounts.credential('MINIMAX_API_KEY'):found.add('minimax')
    if accounts.credential('XAI_MANAGEMENT_API_KEY') and os.environ.get('XAI_TEAM_ID','').strip():found.add('grok')
    if accounts.credential('QODER_CN_API_KEY') and os.environ.get('QODER_CN_ORG_ID','').strip():found.add('qoder-api')
    for key in catalog.KEY_ONLY_APIS:
        if any(accounts.credential(name) for name in catalog.credential_names(key)):found.add(key)
    found.update(k for k in configured_subscriptions if k in catalog.MANUAL_SUBSCRIPTIONS)
    if monitor_url(komari_url):found.add('komari')
    return [key for key,label,group in preferences.MODULES if key in found]


def initialize(directory, detected):
    """Only first installs are initialized. Existing choices survive upgrades."""
    root=Path(directory);target=root/'preferences.json'
    if target.exists():return False
    current=preferences.read(directory)
    chosen=set(detected)
    current['visible']=[key for key,label,group in preferences.MODULES if key in chosen]
    root.mkdir(parents=True,exist_ok=True,mode=0o700)
    import tempfile
    fd,temporary=tempfile.mkstemp(prefix='.discovery-',dir=root)
    try:
        with os.fdopen(fd,'w',encoding='utf-8') as out:json.dump(current,out,ensure_ascii=False)
        try:os.link(temporary,target)
        except FileExistsError:return False  # Don't overwrite a concurrent manual choice.
        return True
    finally:os.unlink(temporary)
