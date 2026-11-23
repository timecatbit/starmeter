"""What 工具更新 checks for each Harness, product by product (an app and its CLI apart), the channel a CLI came from
(without running a package manager), where each vendor publishes its newest version and the command that updates a CLI.
The parity copy of crates/core/src/update_products.rs."""
import json
import os
import platform
import re
import shutil
from pathlib import Path

APP_ELSEWHERE='只在 macOS 上读取 App 的版本'


def _product(id,tool,name,link,kind,apps=(),commands=(),always=True,manual=None,updates_itself=None):
    return {'id':id,'tool':tool,'name':name,'link':link,'kind':kind,'apps':tuple(apps),'commands':tuple(commands),
            'always':always,'manual':manual,'updatesItself':kind=='app' if updates_itself is None else updates_itself}


# In the homepage's order (by Harness), an app before its CLI. OpenCodex keeps a row of its own.
PRODUCTS=[
    _product('magpie','magpie','Magpie','https://github.com/yetone/magpie/releases/latest','app',('magpie','Magpie'),('magpie',),updates_itself=False),
    _product('chatgpt','codex','ChatGPT','https://chatgpt.com/download','app',('ChatGPT',)),
    _product('codex-cli','codex','Codex','https://github.com/openai/codex/releases','cli',commands=('codex',)),
    _product('claude','claude','Claude','https://claude.ai/download','app',('Claude',)),
    _product('claude-code','claude','Claude Code','https://github.com/anthropics/claude-code/releases','cli',commands=('claude',)),
    _product('cursor','cursor','Cursor','https://cursor.com/downloads','app',('Cursor',)),
    _product('cursor-cli','cursor','Cursor CLI','https://cursor.com/cli','cli',commands=('cursor-agent',),always=False),
    _product('deepseek-harness','deepseek-harness','DeepSeek Harness','https://www.npmjs.com/package/@deepseek-ai/dsh','cli',commands=('dsh',)),
    _product('gemini-app','gemini','Gemini App','https://gemini.google/mac/','app',('Gemini',),always=False,manual='由 Google 更新程序自动更新'),
    _product('gemini','gemini','Gemini CLI','https://github.com/google-gemini/gemini-cli/releases','cli',commands=('gemini',)),
    _product('antigravity','gemini','Antigravity','https://antigravity.google/download','app',('Antigravity',),always=False),
    _product('antigravity-ide','gemini','Antigravity IDE','https://antigravity.google/download','app',('Antigravity IDE',),always=False),
    _product('antigravity-cli','gemini','Antigravity CLI','https://antigravity.google/cli','cli',commands=('agy',),always=False),
    _product('opencode-desktop','opencode','OpenCode Desktop','https://opencode.ai/download','app',('OpenCode',),always=False),
    _product('opencode','opencode','OpenCode','https://opencode.ai','cli',commands=('opencode',)),
    _product('qoder','qoder','Qoder','https://qoder.cn/download','app',('Qoder CN','QoderCN'),manual='Qoder CN 没有公开 App 的版本接口'),
    _product('qoder-cli','qoder','Qoder CLI','https://qoder.com.cn/cli','cli',commands=('qoderclicn','qodercn'),always=False),
    _product('workbuddy','workbuddy','WorkBuddy','https://www.workbuddy.cn/','app',('WorkBuddy',)),
]
BY_ID={product['id']:product for product in PRODUCTS}


def product(id):
    return BY_ID.get(id)


def products_of(tool):
    return [product for product in PRODUCTS if product['tool']==tool]


# Versions

def release_version(value):
    match=re.fullmatch(r'v?(\d+(?:\.\d+){0,4})(?:[-+]([a-zA-Z0-9.-]+))?',str(value or '').strip())
    if not match:return None
    parts=tuple(int(n) for n in match[1].split('.'))
    return parts+(0,)*(5-len(parts))


def _prerelease(value):
    text=str(value or '').strip()
    text=text[1:] if text.startswith('v') else text
    core=re.split(r'[-+]',text,maxsplit=1)[0]
    rest=text[len(core):]
    return rest[1:] if rest.startswith('-') else None


def _compare(a,b):
    return (a>b)-(a<b)


def release_order(a,b):
    """-1, 0 or 1 in semantic-version order (a release above its prereleases), None when either can't be read."""
    numbers_a,numbers_b=release_version(a),release_version(b)
    if numbers_a is None or numbers_b is None:return None
    if numbers_a!=numbers_b:return _compare(numbers_a,numbers_b)
    pre_a,pre_b=_prerelease(a),_prerelease(b)
    if pre_a is None and pre_b is None:return 0
    if pre_a is None:return 1
    if pre_b is None:return -1
    parts_a,parts_b=pre_a.split('.'),pre_b.split('.')
    for x,y in zip(parts_a,parts_b):
        if x.isdigit() and y.isdigit():order=_compare(int(x),int(y))
        elif x.isdigit():order=-1
        elif y.isdigit():order=1
        else:order=_compare(x,y)
        if order:return order
    return _compare(len(parts_a),len(parts_b))


def available(id,latest,current):
    """Whether latest is a version this computer hasn't got; Cursor CLI's builds (a date and a commit) by the
    install script's newest one, so another build of the same day is newer too."""
    if id=='cursor-cli':
        a,b=release_version(latest),release_version(current)
        return bool(a is not None and b is not None and (a>b or (a==b and str(latest).strip()!=str(current).strip())))
    return release_order(latest,current)==1


# Channels

def object_file(path):
    try:
        value=json.loads(Path(path).read_text(encoding='utf-8'))
        return value if isinstance(value,dict) else {}
    except (OSError,ValueError):return {}


PACKAGES={'claude-code':('claude-code','','@anthropic-ai/claude-code'),'codex-cli':('codex','codex','@openai/codex'),
          'gemini':('','gemini-cli','@google/gemini-cli'),'deepseek-harness':('','','@deepseek-ai/dsh'),
          'opencode':('','opencode','opencode-ai'),'qoder-cli':('','','@qodercn-ai/qoderclicn'),
          'cursor-cli':('cursor-cli','',''),'antigravity-cli':('antigravity-cli','','')}


def agy_platform():
    system=platform.system()
    os_name='darwin' if system=='Darwin' else 'windows' if system=='Windows' else 'linux'
    arch='arm64' if platform.machine().lower() in ('arm64','aarch64') else 'amd64'
    return os_name+'_'+arch


def own_channel(id):
    sources={'claude-code':('Claude 官方','text','https://downloads.claude.ai/claude-code-releases/latest'),
             'opencode':('官方安装','github','https://api.github.com/repos/anomalyco/opencode/releases/latest'),
             'qoder-cli':('官方安装','qoder-manifest','https://static.qoder.com.cn/qoder-cli-cn/channels/manifest.json'),
             'cursor-cli':('官方安装','cursor-script','https://cursor.com/install'),
             'antigravity-cli':('官方安装','agy-manifest','https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/%s.json'%agy_platform())}
    if id not in sources:return None
    name,kind,url=sources[id]
    return {'channel':name,'channelId':kind,'releaseURL':url}


def _brew(kind,token):
    channel,folder=('Homebrew Cask','cask') if kind=='brew-cask' else ('Homebrew Formula','formula')
    return {'channel':channel,'channelId':kind,'releaseURL':'https://formulae.brew.sh/api/%s/%s.json'%(folder,token)}


def _npm(package):
    return {'channel':'npm','channelId':'npm','releaseURL':'https://registry.npmjs.org/%s/latest'%package}


def cli_channel(id,binary):
    cask,formula,package=PACKAGES.get(id,('','',''))
    path=Path(binary).resolve()
    scope,_,last=package.rpartition('/')
    for parent in path.parents:
        name,above=parent.name,parent.parent.name
        if cask and name==cask and above=='Caskroom':
            receipt=object_file(parent/'.metadata/INSTALL_RECEIPT.json')
            if (receipt.get('source') or {}).get('tap')!='homebrew/cask':return {'channel':'Homebrew 自定义 Cask'}
            return _brew('brew-cask',cask)
        if formula and name==formula and above=='Cellar':
            # The receipt is in the version's folder, the one below Cellar/<formula> on the way to the program.
            version=next((folder for folder in [path]+list(path.parents) if folder.parent==parent),None)
            receipt=object_file(version/'INSTALL_RECEIPT.json') if version else {}
            if (receipt.get('source') or {}).get('tap')!='homebrew/core':return {'channel':'Homebrew 自定义 Formula'}
            return _brew('brew-formula',formula)
        named=(name==last and above==scope) if scope else (bool(package) and name==package and above=='node_modules')
        if named and object_file(parent/'package.json').get('name')==package:return _npm(package)
    # A Windows npm launcher sits beside node_modules; it isn't a symlink into the package.
    if package and path.suffix.lower() in ('.cmd','.bat') and object_file(path.parent.joinpath('node_modules',*package.split('/'),'package.json')).get('name')==package:
        return _npm(package)
    return own_channel(id) or {'channel':'安装渠道待确认'}


# Where each app's newest version is published

# The Tencent sites WorkBuddy's update service answers on: China's first, the one used when the app names none.
WORKBUDDY_SITES=('https://www.workbuddy.cn','https://www.codebuddy.cn','https://copilot.tencent.com','https://www.codebuddy.ai')


def workbuddy_site(app):
    """The site an installed WorkBuddy updates from, as its own product.json names it, only if it is Tencent's."""
    named=object_file(os.path.join(app,'Contents','Resources','app.asar.unpacked','cli','product.json')).get('endpoint') if app else None
    named=named.rstrip('/') if isinstance(named,str) else None
    return named if named in WORKBUDDY_SITES else WORKBUDDY_SITES[0]


def app_source(id,arm=None,app=None):
    arm=platform.machine().lower() in ('arm64','aarch64') if arm is None else arm
    sources={'magpie':('github','https://api.github.com/repos/yetone/magpie/releases/latest'),
             'cursor':('cursor-api','https://cursor.com/api/download?platform=darwin-%s&releaseTrack=stable'%('arm64' if arm else 'x64')),
             'antigravity':('antigravity-releases','https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/releases'),
             'antigravity-ide':('ide-feed','https://antigravity-ide-auto-updater-974169037036.us-central1.run.app/api/update/%s/stable/latest'%('darwin-arm64' if arm else 'darwin')),
             'opencode-desktop':('github','https://api.github.com/repos/anomalyco/opencode/releases/latest'),
             # The update service WorkBuddy's download page asks for its download.
             'workbuddy':('workbuddy-feed','%s/v2/update?platform=workbuddy-darwin-%s'%(workbuddy_site(app),'arm64' if arm else 'x64'))}
    return sources.get(id)


def ide_release(body):
    """Antigravity IDE's feed: the version from its download folder (…/stable/2.8.0-<build>/…) or its product version,
    and the commit it was built from."""
    try:data=json.loads(body)
    except ValueError:return None
    if not isinstance(data,dict):return None
    commit=data.get('version')
    if not isinstance(commit,str) or not commit:return None
    url=data.get('url') if isinstance(data.get('url'),str) else ''
    match=re.search(r'/stable/(\d+(?:\.\d+){1,3})-',url)
    version=match[1] if match else data.get('productVersion') if isinstance(data.get('productVersion'),str) else None
    return (version,commit) if version is not None else None


def _text(value):
    """A JSON value as Python's str() writes it, which the Rust copy follows."""
    return str(value)


def latest_version(kind,body):
    """The newest version in a source's answer; KeyError or ValueError when it names none."""
    if kind=='text':return body.decode('utf-8','replace').strip()
    if kind=='cursor-script':
        match=re.search(r'downloads\.cursor\.com/lab/([0-9][0-9A-Za-z.-]*)/',body.decode('utf-8','replace'))
        if not match:raise KeyError('version')
        return match[1]
    data=json.loads(body)
    def field(name):
        value=data.get(name) if isinstance(data,dict) else None
        if not value:raise KeyError(name)
        return _text(value)
    if kind=='brew-formula':return _text(data['versions']['stable'])
    if kind=='brew-cask':return field('version').split(',')[0]
    if kind=='github':
        tag=field('tag_name');return tag[1:] if tag.startswith('v') else tag
    if kind=='qoder-manifest':return field('latest')
    if kind=='antigravity-releases':
        versions=[row.get('version') for row in (data if isinstance(data,list) else []) if isinstance(row,dict) and release_version(row.get('version')) is not None]
        if not versions:raise KeyError('version')
        best=versions[0]
        for version in versions[1:]:
            if release_order(version,best)==1:best=version
        return _text(best)
    if kind=='workbuddy-feed':
        # 5.11.3.43806934: the app's own version is the first three, the fourth its build.
        value=data.get('version') or data.get('productVersion') if isinstance(data,dict) else None
        if not value:raise KeyError('version')
        return '.'.join(_text(value).split('.')[:3])
    if kind=='ide-feed':
        found=ide_release(body)
        if not found:raise KeyError('version')
        return found[0]
    return field('version')


def app_commit(app):
    commit=object_file(os.path.join(app,'Contents','Resources','app','product.json')).get('commit')
    return commit if isinstance(commit,str) and commit else None


# Updating a CLI

def manager(binary,name):
    path=Path(binary).resolve()
    folders=('Caskroom','Cellar') if name=='brew' else ('node_modules',)
    found=next((folder for folder in [path]+list(path.parents) if folder.name in folders),None)
    candidates=[]
    if found is not None:
        prefix=found.parent
        if name=='brew':candidates=[prefix/'bin/brew']
        elif prefix.name=='lib':candidates=[prefix.parent/'bin/npm']
        else:candidates=[prefix/'npm.cmd',prefix/'bin/npm']
    return next((str(candidate) for candidate in candidates if candidate.is_file()),None) or shutil.which(name)


# The CLI's own update, for its vendor's installer or an install from somewhere unknown (the CLI tells it apart itself).
OWN_UPDATES={'claude-code':'update','codex-cli':'update','cursor-cli':'update','antigravity-cli':'update','qoder-cli':'update','opencode':'upgrade'}


def update_plan(id,binary,channel):
    cask,formula,package=PACKAGES.get(id,('','',''))
    kind=channel.get('channelId') or ''
    if kind in ('brew-cask','brew-formula'):
        brew=manager(binary,'brew')
        if not brew:return None
        return [brew,'upgrade','--cask',cask] if kind=='brew-cask' else [brew,'upgrade',formula]
    if kind=='npm':
        npm=manager(binary,'npm')
        return [npm,'install','-g',package+'@latest'] if npm else None
    if (channel.get('channel') or '').startswith('Homebrew'):return None
    return [binary,OWN_UPDATES[id]] if id in OWN_UPDATES else None


def plan_text(plan):
    program=Path(plan[0]).name
    for suffix in ('.cmd','.exe'):
        if program.endswith(suffix):program=program[:-len(suffix)];break
    return ' '.join([program]+list(plan[1:]))
