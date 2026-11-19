#!/usr/bin/env python3
"""Package a release zip with a prebuilt panel and engine, so users install without git or a Rust toolchain."""
import argparse
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import quote

from build import build, build_engine

ROOT = Path(__file__).resolve().parents[1]
ENTRY = 'starmeter.30s.sh'
REPOSITORY = 'https://github.com/timecatbit/starmeter'
# The installer runs from the package root with the prebuilt panel and engine beside it; lib/ keeps only data.
CONTENTS = [ENTRY, 'lib', 'assets', 'config.example.sh', 'README.md', 'CHANGELOG.md', 'LICENSE', 'scripts/install.py']
APP = Path('build/OpenCodexPanel.app')
ENGINE = Path('build/starmeter-engine')


def version():
    plugin = re.search(r'^VERSION = "([^"]+)"$', (ROOT / 'lib/swiftbar_plugin.py').read_text(), re.M)
    engine = re.search(r'^version = "([^"]+)"$', (ROOT / 'Cargo.toml').read_text(), re.M)
    entry = re.search(r'<xbar.version>([^<]+)</xbar.version>', (ROOT / ENTRY).read_text())
    if not plugin or not entry or not engine or len({plugin.group(1), entry.group(1), engine.group(1)}) != 1:
        raise SystemExit('VERSION in lib/swiftbar_plugin.py, the workspace version in Cargo.toml and xbar.version in %s must match.' % ENTRY)
    return plugin.group(1)


def downloads(number):
    """What a release offers, in the order GitHub lists it (by file name): file and the English label the page
    shows instead of the name. In-app updates look these names up, and the SwiftBar plugin keeps the short
    name every installed copy still fetches."""
    name = 'token-monitor-' + number
    return [
        (name + '-macos-apple-silicon.dmg', 'Mac · Apple Silicon'),
        (name + '-macos-intel.dmg', 'Mac · Intel'),
        (name + '-windows-x64-setup.exe', 'Windows · Installer'),
        (name + '-windows-x64.zip', 'Windows · Portable'),
        (name + '.zip', 'Mac · SwiftBar plugin'),
    ]


def release_notes(number):
    """Only what someone downloading needs. The changes live in CHANGELOG.md and the full download guide in
    the README; the release links to both, pinned to this tag, instead of repeating them."""
    if not re.search(r'^## %s$' % re.escape(number), (ROOT / 'CHANGELOG.md').read_text(), re.M):
        raise SystemExit('CHANGELOG.md has no section for ' + number)
    tree = '%s/blob/v%s/' % (REPOSITORY, number)
    # GitHub drops the dots from a heading's anchor: "## 5.23.0" becomes #5230.
    return ('在下方附件中选一个文件：Mac 按芯片选 **Mac · Apple Silicon** 或 **Mac · Intel**，Windows 推荐 **Windows · Installer**，'
            '已经在用 SwiftBar 的选 **Mac · SwiftBar plugin**。怎么选、怎么安装见 [下载与安装](%sREADME.md#%s)。\n\n'
            'Mac 独立版预览未做 Apple 公证，首次打开被拦时到「系统设置 → 隐私与安全性 → 仍要打开」。\n\n'
            '本版改动见 [更新记录](%sCHANGELOG.md#%s)。\n'
            % (tree, quote('下载与安装'), tree, number.replace('.', '')))


def verify(archive, name):
    """Unpack like a user would and check the files and signature the installer relies on."""
    with tempfile.TemporaryDirectory(prefix='starmeter-verify-') as scratch:
        subprocess.run(['ditto', '-x', '-k', str(archive), scratch], check=True)
        package = Path(scratch) / name
        for relative in ['install.py', ENTRY, 'lib/codex-pricing.json', 'lib/model-pricing.json', 'lib/CCUSAGE_LICENSE',
                         'assets/logos/LICENSE', str(APP / 'Contents/Info.plist'), str(ENGINE)]:
            if not (package / relative).is_file():
                raise SystemExit('Package is missing ' + relative)
        if (list(package.rglob('__pycache__')) or list(package.rglob('.DS_Store')) or list(package.rglob('*.py')) != [package / 'install.py']
                or (package / 'panel').exists() or (package / 'engine').exists()):
            raise SystemExit('Package contains development files.')
        subprocess.run(['codesign', '--verify', '--strict', str(package / ENGINE)], check=True)
        engine = subprocess.run(['lipo', '-archs', str(package / ENGINE)], capture_output=True, text=True, check=True).stdout.split()
        if sorted(engine) != ['arm64', 'x86_64']:
            raise SystemExit('Engine binary is not universal: ' + ' '.join(engine))
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(package / APP)], check=True)
        architectures = subprocess.run(['lipo', '-archs', str(package / APP / 'Contents/MacOS/OpenCodexPanel')],
                                       capture_output=True, text=True, check=True).stdout.split()
        if sorted(architectures) != ['arm64', 'x86_64']:
            raise SystemExit('Panel binary is not universal: ' + ' '.join(architectures))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', help='Fail unless the tag names this version, for example v5.12.0.')
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    number = version()
    if args.tag and args.tag != 'v' + number:
        raise SystemExit('Tag %s does not match version %s.' % (args.tag, number))
    notes = release_notes(number)
    name = 'token-monitor-' + number
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (name + '.zip')
    with tempfile.TemporaryDirectory(prefix='starmeter-release-') as scratch:
        package = Path(scratch) / name
        package.mkdir()
        for relative in CONTENTS:
            source, target = ROOT / relative, package / Path(relative).name
            if relative == 'lib':
                shutil.copytree(source, target, ignore=lambda folder, names: [n for n in names if not (n.endswith('.json') or n.endswith('LICENSE'))])
            elif source.is_dir():
                shutil.copytree(source, target, ignore=shutil.ignore_patterns('__pycache__', '*.pyc', '.DS_Store'))
            else:
                shutil.copy2(source, target)
        build(package / APP, universal=True)
        build_engine(package / ENGINE, universal=True)
        # ditto keeps the bundle's signature and executable bits; no AppleDouble or quarantine data.
        archive.unlink(missing_ok=True)
        subprocess.run(['ditto', '-c', '-k', '--keepParent', '--norsrc', '--noextattr', '--noacl',
                        str(package), str(archive)], check=True)
    verify(archive, name)
    (output / 'release-notes.md').write_text(notes)
    # What the publish step uploads, as gh's file#label arguments.
    (output / 'release-assets.txt').write_text(''.join('%s#%s\n' % (file, label) for file, label in downloads(number)))
    print(archive)


if __name__ == '__main__':
    main()
