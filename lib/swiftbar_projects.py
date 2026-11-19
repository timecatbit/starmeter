"""Which project a request belongs to, by the name the user gave its folder; only that name is kept, never the path.

Claude (and Qoder CN, which writes Claude's format) files each conversation under projects/<folder>, the folder
written with everything but letters and digits as '-': the folder the conversation was started in, or moved to in
the Claude app. That folder is the project, whatever folders the conversation went on to work in (a subfolder, a
scratchpad). A worktree Claude makes for a conversation (<repository>/.claude/worktrees/<name>) belongs to its
repository, and the Claude app's conversations without a folder (its scratch workspaces) are Claude 对话.
"""
import os
import re

CLAUDE_CHATS = 'Claude 对话'
FOLDERS = {}


def encoded(name):
    """Claude's name for a folder: every character but an ASCII letter or digit becomes '-'."""
    return ''.join(c if c.isascii() and c.isalnum() else '-' for c in name)


def decode(name):
    """The folder a name Claude gave it stands for, looked up on disk: the name alone cannot tell a '-' that was '/'
    from one that was a space, a dot or a dash. The longest matching name is tried first at each level."""
    at = name.find('--claude-worktrees-')
    if at >= 0:name = name[:at]
    if name.startswith('-'):root, rest = '/', name[1:]
    elif len(name) > 3 and name[0].isascii() and name[0].isalpha() and name[1:3] == '--':root, rest = name[0] + ':/', name[3:]
    else:return None
    return walk(root, rest, 0) if rest else None


def walk(folder, rest, depth):
    if depth > 40:return None
    try:entries = os.listdir(folder)
    except OSError:return None
    names = [(encoded(entry), entry) for entry in entries]
    names = sorted(((code, entry) for code, entry in names if code and (rest == code or rest.startswith(code + '-'))),
                   key=lambda pair: (-len(pair[0]), pair[1]))
    for code, entry in names:
        path = os.path.join(folder, entry)
        if not os.path.isdir(path):continue
        if code == rest:return path
        found = walk(path, rest[len(code) + 1:], depth + 1)
        if found:return found
    return None


def folder_project(name):
    """The project a Claude conversation folder name stands for."""
    if name not in FOLDERS:
        FOLDERS[name] = CLAUDE_CHATS if '-Library-Application-Support-Claude-scratch-workspaces-' in name else (
            name_of(path) if (path := decode(name)) else None)
    return FOLDERS[name]


def transcript_project(file):
    """The project of a Claude-format transcript, by the folder it is filed under (…/projects/<folder>/…); none when
    that folder no longer stands for one on disk."""
    parts = re.split(r'[\\/]', str(file))
    at = max((i for i, part in enumerate(parts) if part == 'projects'), default=None)
    if at is None or at + 2 >= len(parts):return None
    return folder_project(parts[at + 1])


def name_of(path):
    """The project of a folder a request ran in: the folder's last name, except a Claude worktree's repository, a
    Claude scratchpad's conversation folder, and Claude 对话 for the Claude app's scratch workspaces."""
    path = path.strip().replace('\\', '/').rstrip('/')
    at = path.find('/.claude/worktrees/')
    if at >= 0:path = path[:at]
    if '/Library/Application Support/Claude/scratch-workspaces/' in path:return CLAUDE_CHATS
    scratch = re.match(r'(?:/private)?/tmp/claude-[0-9]+/([^/]+)', path)
    if scratch:return folder_project(scratch.group(1))
    return path.rsplit('/', 1)[-1] or None
