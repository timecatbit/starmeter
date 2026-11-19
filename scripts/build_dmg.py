#!/usr/bin/env python3
"""Package the Starmeter menu bar app for one chip as a disk image: the app with its engine, price tables and logos,
an Applications shortcut and a short guide; then mount it, check the signature and smoke-test the engine inside."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

from build import build_app, build_engine
from package import downloads, version

ROOT = Path(__file__).resolve().parents[1]
# The file name ends with the chip, as the release lists it; the volume name says the same in plain words.
ARCHES = {
    'arm64': ('aarch64-apple-darwin', '-macos-apple-silicon.dmg', 'Apple 芯片（M 系列）', 'Apple Silicon'),
    'x86_64': ('x86_64-apple-darwin', '-macos-intel.dmg', 'Intel 处理器', 'Intel'),
}


def smoke(app, root):
    """The engine the app ships, with a private home and settings and no automatic network checks."""
    resources = app / 'Contents/Resources'
    home = root / 'home'
    home.mkdir()
    environment = {'HOME': str(home), 'PATH': '/usr/bin:/bin', 'OCX_SELF_UPDATES': '0', 'OCX_PRICING_UPDATES': '0',
                   'TOKEN_MONITOR_SMOKE_TEST': '1', 'OCX_MAC_APP': '1', 'OCX_PLUGIN_PATH': str(resources / 'starmeter'),
                   'OCX_STATE_DIR': str(root / 'private-state'), 'OCX_CACHE_DIR': str(root / 'private-cache')}
    subprocess.run([str(resources / 'starmeter-engine'), '--worker', 'smoke-test'], env=environment, check=True, timeout=60)


def guide(release, chip, notarized):
    return (f'Starmeter {release} · macOS 独立版' + ('' if notarized else '预览') + '\n\n'
            f'此镜像适用于：{chip}，macOS 13 或更新版本。\n'
            '不知道机型？苹果菜单  → 关于本机，查看「芯片」或「处理器」。\n\n'
            '1. 把 Starmeter 拖到 Applications（应用程序）。\n'
            '2. 退出镜像，在「应用程序」打开 Starmeter。菜单栏会出现星星。\n'
            '3. 左键看用量，右键管理模块、登录启动和通知。\n\n'
            '从 Token Monitor 升级？Starmeter 是它的新名字。先退出旧版并把 Token Monitor 移到废纸篓，再打开 Starmeter，'
            '需要时在右键菜单重新勾选“登录时启动”。设置和 Key 都会保留。\n\n'
            '已经装过 SwiftBar 插件？设置和 Key 仍在本机，可先停用该插件，再打开独立版。\n'
            '想继续用 SwiftBar？请下载发布页里的 Mac · SwiftBar plugin，不用安装此镜像。\n\n'
            + ('此包由 Developer ID 签名并提交 Apple 公证。\n' if notarized else
               '此预览未做 Apple 公证。若首次打开被拦，确认镜像下载自项目的 GitHub 发布页后，'
               '到「系统设置 → 隐私与安全性 → 仍要打开」。公司管理的 Mac 可能不允许放行。\n'))


def dmg(output, identity, notary, arch):
    triple, suffix, chip, volume_name = ARCHES[arch]
    release = version()
    output.mkdir(parents=True, exist_ok=True)
    artifact = output / next(file for file, _ in downloads(release) if file.endswith(suffix))
    with tempfile.TemporaryDirectory(prefix='starmeter-dmg-') as temp:
        root = Path(temp)
        engine = build_engine(root / 'engine' / 'starmeter-engine', target=triple)
        volume = root / 'volume'
        volume.mkdir()
        app = build_app(volume / 'Starmeter.app', 'app', target=triple, identity=identity, engine=engine)
        (volume / 'Applications').symlink_to('/Applications')
        (volume / 'README.txt').write_text(guide(release, chip, bool(notary)), encoding='utf-8')
        target = root / artifact.name
        subprocess.run(['hdiutil', 'create', '-ov', '-format', 'UDZO', '-volname', f'Starmeter ({volume_name})',
                        '-srcfolder', str(volume), str(target)], check=True)
        if identity != '-':
            subprocess.run(['codesign', '--force', '--sign', identity, '--timestamp', str(target)], check=True)
        if notary:
            subprocess.run(['xcrun', 'notarytool', 'submit', str(target), '--keychain-profile', notary, '--wait'], check=True)
            subprocess.run(['xcrun', 'stapler', 'staple', str(target)], check=True)
        subprocess.run(['hdiutil', 'verify', str(target)], check=True)
        mount = root / 'mount'
        mount.mkdir()
        subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', str(mount), str(target)], check=True)
        try:
            installed = root / 'installed' / 'Starmeter.app'
            installed.parent.mkdir()
            subprocess.run(['ditto', str(mount / 'Starmeter.app'), str(installed)], check=True)
            subprocess.run(['codesign', '--verify', '--deep', '--strict', str(installed)], check=True)
            if not (mount / 'README.txt').is_file() or os.readlink(mount / 'Applications') != '/Applications':
                raise SystemExit('Disk image is missing the installation guide or Applications shortcut')
            binary = installed / 'Contents/MacOS/Starmeter'
            found = subprocess.run(['lipo', '-archs', str(binary)], capture_output=True, text=True, check=True).stdout.split()
            if found != [arch]:
                raise SystemExit('Starmeter.app is built for %s, not %s' % (' '.join(found), arch))
            smoke(installed, root)
        finally:
            subprocess.run(['hdiutil', 'detach', str(mount)], check=True)
        shutil.move(str(target), str(artifact))
    print(artifact)
    return artifact


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--tag', default='')
    parser.add_argument('--arch', choices=sorted(ARCHES), default=platform.machine(), help="The chip; defaults to this Mac's.")
    parser.add_argument('--identity', default=os.environ.get('TOKEN_MONITOR_SIGN_IDENTITY', '-'))
    parser.add_argument('--notary-profile', default=os.environ.get('TOKEN_MONITOR_NOTARY_PROFILE', ''))
    args = parser.parse_args()
    if args.tag and args.tag != 'v' + version():
        parser.error('tag and project version do not match')
    if args.notary_profile and args.identity == '-':
        parser.error('notarization requires a Developer ID signature')
    dmg(args.output.resolve(), args.identity, args.notary_profile, args.arch)


if __name__ == '__main__':
    main()
