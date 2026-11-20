"""Explicit connection overrides; no copied credentials or inferred monitor addresses."""
import ctypes
import errno
import os
from pathlib import Path
import re
import shlex
import sys
import tempfile
from urllib.parse import urlsplit
import swiftbar_catalog as catalog

KEYS = {'OCX_HOME', 'OCX_BASE_URL', 'OCX_TOKEN_FILE', 'OCX_CHATGPT_HOME', 'KOMARI_BASE_URL', 'DEEPSEEK_API_KEY', 'DEEPSEEK_API_KEY_FILE',
        'XAI_MANAGEMENT_API_KEY', 'XAI_MANAGEMENT_API_KEY_FILE', 'XAI_TEAM_ID',
        'MINIMAX_API_KEY', 'MINIMAX_API_KEY_FILE', 'MINIMAX_REGION', 'MOONSHOT_REGION', 'QODER_CN_ORG_ID'}
KEYS.update(name + suffix for key in catalog.API_KEYS for name in catalog.credential_names(key) for suffix in ('', '_FILE'))
HEADER = '''# 本机高级设置：仅本用户可读，不随插件上传或覆盖。
# 常用设置请在「主页模块」悬停对应来源；本机客户端默认自动读取。
# 注释是说明，不代表已配置。无需填写即可使用已发现的本机客户端。
# API 连接只保存密钥文件路径；在插件中输入的 Key 存于同目录 keys/，仅本用户可读。
'''


def monitor_url(value):
    value = str(value or '').strip().rstrip('/')
    try:
        parsed = urlsplit(value)
        host = (parsed.hostname or '').lower()
        if (parsed.scheme not in ('http', 'https') or not host or parsed.username or
                parsed.password or parsed.query or parsed.fragment or
                host.endswith('.example') or host in ('example.com', 'example.org', 'your-komari.example')):
            return ''
        parsed.port  # Reject invalid ports as well.
        return value
    except ValueError:
        return ''


def write_overrides(directory, values):
    """Override only selected keys, preserving unrelated advanced shell settings."""
    if not values or not set(values) <= KEYS or not all(isinstance(v, str) and '\n' not in v and '\x00' not in v for v in values.values()):
        raise ValueError('invalid connection settings')
    root = Path(directory); root.mkdir(parents=True, exist_ok=True, mode=0o700)
    path = root/'config.local.sh'
    text = path.read_text() if path.exists() else HEADER
    # Keep earlier advanced expressions intact. Last export wins, including an explicit empty value.
    for key, value in values.items():
        text = re.sub(r'^export '+re.escape(key)+r'=.* # managed by SwiftBarToken\n?', '', text, flags=re.M)
        text = text.rstrip()+'\nexport '+key+'='+shlex.quote(value)+' # managed by SwiftBarToken\n'
    fd, temporary = tempfile.mkstemp(prefix='.config-', dir=root)
    try:
        with os.fdopen(fd, 'w') as out: out.write(text)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary): os.unlink(temporary)


KEY_FORMAT = re.compile(r'[\x21-\x7e]{8,1024}')


def managed_key(directory, filename):
    """Only keys typed into the plugin are ours to delete; chosen files are never removed."""
    try:
        return Path(filename).expanduser().resolve().parent == (Path(directory) / 'keys').resolve()
    except (OSError, RuntimeError):
        return False


def ensure_file(directory):
    root = Path(directory); root.mkdir(parents=True, exist_ok=True, mode=0o700)
    path = root/'config.local.sh'
    try:
        fd = os.open(path, os.O_CREAT|os.O_EXCL|os.O_WRONLY, 0o600)
    except FileExistsError:
        return path
    with os.fdopen(fd, 'w') as out: out.write(HEADER)
    return path


def migrate_config(destination, state, explicit=None):
    """An existing user file always wins, whatever it holds. Never silently reimport checkout config. The copy is read
    and written in full beside the target before it takes the target's name: a failed read or write leaves nothing
    there and the next install migrates again. The name is taken by an operation that never replaces a file; when the
    volume has none, the legacy file stays where it was and the install reports it."""
    target = Path(state)/'config.local.sh'
    legacy = Path(destination)/'.opencodex-panel/config.local.sh'
    source = Path(explicit).expanduser() if explicit else legacy
    if explicit and target.exists():
        raise ValueError('本机配置已存在；请在连接设置中修改，不覆盖现有文件。')
    if not target.exists() and source.is_file():
        contents = source.read_bytes()
        target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, temporary = tempfile.mkstemp(prefix='.config-', dir=target.parent)
        try:
            with os.fdopen(fd, 'wb') as out:
                out.write(contents); out.flush(); os.fsync(out.fileno())
            os.chmod(temporary, 0o600)
            publish_config(temporary, target)
        finally:
            if os.path.exists(temporary): os.unlink(temporary)
    # Installer already backed up the legacy file before this migration.
    if legacy.exists(): legacy.unlink()
    return target


def publish_config(temporary, target):
    """The finished copy takes the target's name in one step that never replaces a file: a hard link, or on a volume
    without hard links the system's exclusive rename. A file that appeared there meanwhile wins either way, with no window
    between a check and the rename. A volume with neither operation leaves the copy unpublished: the caller keeps the
    legacy file and the next install migrates again."""
    try:
        os.link(temporary, target)
        return
    except FileExistsError:
        return
    except OSError:
        pass
    try:
        rename_exclusive(temporary, target)
    except FileExistsError:
        pass
    except OSError as error:
        if error.errno not in NO_EXCLUSIVE_RENAME: raise
        raise OSError(error.errno, '本机配置没有迁移：这个卷既不支持硬链接，也不支持不覆盖已有文件的改名，复制件无法独占发布。'
                      '旧配置原样保留，重新安装会再次迁移。') from error


# The system's rename that refuses an existing target: macOS renamex_np with RENAME_EXCL, Linux renameat2 with
# RENAME_NOREPLACE; and the errors a system or volume without it gives.
RENAME_EXCL = 0x4
RENAME_NOREPLACE = 0x1
AT_FDCWD = -100
NO_EXCLUSIVE_RENAME = {errno.ENOTSUP, errno.EOPNOTSUPP, errno.EINVAL, errno.ENOSYS}


def rename_exclusive(source, target):
    """Rename source to target, refusing with FileExistsError when target exists: the check and the rename are one
    operation in the kernel. OSError with an errno in NO_EXCLUSIVE_RENAME when this system or volume has no such rename."""
    if sys.platform == 'darwin':
        name = 'renamex_np'
        signature = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
        arguments = (os.fsencode(source), os.fsencode(target), RENAME_EXCL)
    elif sys.platform.startswith('linux'):
        name = 'renameat2'
        signature = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
        arguments = (AT_FDCWD, os.fsencode(source), AT_FDCWD, os.fsencode(target), RENAME_NOREPLACE)
    else:
        raise OSError(errno.ENOTSUP, 'no exclusive rename on ' + sys.platform)
    try:
        rename = getattr(ctypes.CDLL(None, use_errno=True), name)
    except (AttributeError, OSError):
        raise OSError(errno.ENOSYS, name + ' is not available on this system') from None
    rename.argtypes, rename.restype = signature, ctypes.c_int
    if rename(*arguments) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), str(source), None, str(target))
