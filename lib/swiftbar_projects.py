"""Which project a request belongs to, by the name the user gave its folder; only that name is kept, never the path.

Claude (and Qoder CN, which writes Claude's format) files each conversation under projects/<folder>, the folder
written with everything but letters and digits as '-': the folder the conversation was started in, or moved to in
the Claude app. That folder is the project, whatever folders the conversation went on to work in (a subfolder, a
scratchpad). A worktree Claude makes for a conversation (<repository>/.claude/worktrees/<name>) belongs to its
repository, and the Claude app's conversations without a folder (its scratch workspaces) are Claude 对话.

The written name cannot tell a '-' that was '/' from a space, a dot or a dash, and the folders themselves are never
looked at (reading Documents or Desktop makes macOS ask the user for access): the path comes from the folders Claude
Code lists in its own settings (only the keys of `projects` in ~/.claude.json) or from the working folders the
conversation's first lines record.
"""
import json
import os
import re

CLAUDE_CHATS = 'Claude 对话'
HEAD = 256 * 1024
SETTINGS = [None, {}]
TRANSCRIPTS = {}


def encoded(name):
    """Claude's name for a folder: every character but an ASCII letter or digit becomes '-'."""
    return ''.join(c if c.isascii() and c.isalnum() else '-' for c in name)


def repository(path):
    """A path without the Claude worktree it is in (<repository>/.claude/worktrees/<name>/…)."""
    at = path.find('/.claude/worktrees/')
    return path[:at] if at >= 0 else path


def settings_file(configured):
    """Claude Code's settings file: in the first folder CLAUDE_CONFIG_DIR names (several may be listed, comma
    separated, starting with ~), else in the home folder."""
    configured = (configured or '').strip()
    if configured:return os.path.join(os.path.expanduser(configured.split(',')[0].strip()), '.claude.json')
    return os.path.join(os.path.expanduser('~'), '.claude.json')


def listed(written):
    """The folders Claude Code has run in, by their written name: the keys of `projects` in its settings, each also under
    its repository's name when it is a worktree. Read again when the file changes."""
    file = settings_file(os.environ.get('CLAUDE_CONFIG_DIR'))
    try:stamp = (file, os.stat(file).st_mtime_ns)
    except OSError:stamp = (file, None)
    if SETTINGS[0] != stamp:
        folders = {}
        try:
            with open(file, 'rb') as handle:projects = json.loads(handle.read()).get('projects')
        except (OSError, ValueError, AttributeError):projects = None
        for path in projects if isinstance(projects, dict) else {}:
            path = path.replace('\\', '/').rstrip('/'); root = repository(path)
            folders.setdefault(encoded(root), root); folders.setdefault(encoded(path), path)
        SETTINGS[:] = [stamp, folders]
    return SETTINGS[1].get(written)


def target(folder):
    """The written name a conversation folder stands for, without the worktree part; None for the Claude app's
    conversations without a folder."""
    if '-Library-Application-Support-Claude-scratch-workspaces-' in folder:return None
    at = folder.find('--claude-worktrees-')
    return folder[:at] if at >= 0 else folder


def folder_project(folder):
    """The project a Claude conversation folder name stands for, when Claude Code's settings list it."""
    written = target(folder)
    if written is None:return CLAUDE_CHATS
    path = listed(written)
    return name_of(path) if path else None


def within(cwd, folder):
    """The folder of a working path that a conversation folder name stands for: the path itself or one of the folders it
    is in. The project's name then, without reading anything."""
    written = target(folder)
    if written is None:return None
    path = cwd.strip().replace('\\', '/')
    end = len(path.rstrip('/'))
    while True:
        prefix = path[:end]
        if prefix and encoded(prefix) == written:return name_of(prefix)
        end = prefix.rfind('/')
        if end < 0:return None


def transcript_folder(file):
    """The conversation folder a transcript is filed under (…/projects/<folder>/…)."""
    parts = re.split(r'[\\/]', str(file))
    at = max((i for i, part in enumerate(parts) if part == 'projects'), default=None)
    return parts[at + 1] if at is not None and at + 2 < len(parts) else None


def first_folders(file):
    """The working folders a transcript's first lines record, in order."""
    try:
        with open(file, 'rb') as handle:data = handle.read(HEAD)
    except OSError:return []
    lines = data.split(b'\n')
    if len(data) >= HEAD:lines = lines[:-1]
    found = []
    for line in lines:
        try:row = json.loads(line)
        except ValueError:continue
        cwd = row.get('cwd') if isinstance(row, dict) else None
        if isinstance(cwd, str) and cwd.strip():found.append(cwd)
    return found


def transcript_project(file):
    """The project of a Claude-format transcript, by the folder it is filed under: as Claude Code's settings list it,
    else as the working folders its first lines record show it; None when neither says."""
    file = str(file)
    if file in TRANSCRIPTS:return TRANSCRIPTS[file]
    folder = transcript_folder(file)
    if folder is None:return None
    found = folder_project(folder) or next((name for name in (within(cwd, folder) for cwd in first_folders(file)) if name), None)
    if found:TRANSCRIPTS[file] = found
    return found


def name_of(path):
    """The project of a folder a request ran in: the folder's last name, except a Claude worktree's repository, a
    Claude scratchpad's conversation folder, and Claude 对话 for the Claude app's scratch workspaces."""
    path = repository(path.strip().replace('\\', '/').rstrip('/'))
    if '/Library/Application Support/Claude/scratch-workspaces/' in path:return CLAUDE_CHATS
    scratch = re.match(r'(?:/private)?/tmp/claude-[0-9]+/([^/]+)', path)
    if scratch:return folder_project(scratch.group(1))
    return path.rsplit('/', 1)[-1] or None
