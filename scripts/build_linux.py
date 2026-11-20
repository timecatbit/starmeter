#!/usr/bin/env python3
"""Package the Linux tray as an AppImage and a .deb, with the engine, price tables and logos among the app's
resources (the .deb also installs the starmeter command), and the starmeter command on its own; smoke-test the
engine first. The release publishes them beside the Mac and Windows downloads."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from build import ROOT, build_app, build_engine
from build_windows import smoke
from package import command_line_archive, verify_command_line, version


def check_deb(package):
    """The .deb installs /usr/bin/starmeter, executable, and the engine where that script runs it from."""
    listing = subprocess.run(['dpkg-deb', '-c', str(package)], capture_output=True, text=True, check=True).stdout.splitlines()
    # dpkg-deb lists the paths as the archive holds them: with a leading ./ or, in Tauri's .deb, without.
    entries = {line.split()[-1].removeprefix('./'): line.split()[0] for line in listing if line.strip()}
    command = entries.get('usr/bin/starmeter', '')
    if not command.startswith('-rwx') or 'usr/lib/Starmeter/bin/starmeter-engine' not in entries:
        raise SystemExit('The .deb lacks /usr/bin/starmeter or the engine it runs:\n' + '\n'.join(listing))
    script = (ROOT / 'app/src-tauri/linux/starmeter').read_text(encoding='utf-8')
    if '/usr/lib/Starmeter/bin/starmeter-engine cli' not in script:
        raise SystemExit('/usr/bin/starmeter does not run the engine the .deb ships.')


def without_wayland(appdir, output):
    """The AppImage again from Tauri's AppDir, without the Wayland client libraries the build machine's were: the
    computer's own match its graphics drivers, and Ubuntu 22.04's beside a newer system's Mesa left WebKit with no EGL
    display (EGL_BAD_PARAMETER) and the panel a black window. Tauri has no way to exclude a library, so the AppImage is
    made a second time with linuxdeploy, Tauri's own copy, told to leave them out."""
    for library in (appdir / 'usr' / 'lib').glob('libwayland-*.so*'):
        library.unlink()
    tools = Path(os.environ.get('XDG_CACHE_HOME') or Path.home() / '.cache') / 'tauri'
    environment = dict(os.environ, ARCH='x86_64', LDAI_OUTPUT=str(output), APPIMAGE_EXTRACT_AND_RUN='1')
    output.unlink(missing_ok=True)
    subprocess.run([str(tools / 'linuxdeploy-x86_64.AppImage'), '--appdir', str(appdir), '--exclude-library', 'libwayland-*',
                    '--output', 'appimage'], cwd=tools, env=environment, check=True, stdout=subprocess.DEVNULL)
    check_appimage(output)


def check_appimage(image):
    """The AppImage carries no Wayland client library of its own (see without_wayland)."""
    with tempfile.TemporaryDirectory(prefix='starmeter-appimage-') as scratch:
        subprocess.run([str(image), '--appimage-extract'], cwd=scratch, check=True, stdout=subprocess.DEVNULL,
                       env=dict(os.environ, APPIMAGE_EXTRACT_AND_RUN='1'))
        carried = sorted(path.name for path in (Path(scratch) / 'squashfs-root' / 'usr' / 'lib').glob('libwayland-*.so*'))
        if carried:
            raise SystemExit('The AppImage carries Wayland client libraries: ' + ', '.join(carried))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--tag', default='')
    args = parser.parse_args()
    if not sys.platform.startswith('linux'):
        raise SystemExit('The Linux tray is built on Linux.')
    release = version()
    if args.tag and args.tag != 'v' + release:
        raise SystemExit('Tag %s does not match version %s.' % (args.tag, release))
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix='starmeter-linux-'))
    engine = build_engine(scratch / 'engine' / 'starmeter-engine')
    folder = build_app(scratch / 'Starmeter', 'app', bundles=['appimage', 'deb'], engine=engine)
    smoke(folder, scratch / 'smoke', 'starmeter-engine')
    bundle = ROOT / 'target' / 'release' / 'bundle'
    name = 'starmeter-%s-linux-x64' % release
    for kind, suffix in (('appimage', '.AppImage'), ('deb', '.deb')):
        built = sorted((bundle / kind).glob('*' + suffix))
        if len(built) != 1:
            raise SystemExit('Expected one %s, found %d.' % (suffix, len(built)))
        if kind == 'appimage':
            without_wayland(bundle / kind / 'Starmeter.AppDir', output / (name + suffix))
        else:
            shutil.copy2(built[0], output / (name + suffix))
        print(output / (name + suffix))
    check_deb(output / (name + '.deb'))
    verify_command_line(command_line_archive(engine, output, name + '-cli.tar.gz'))
    shutil.rmtree(scratch, ignore_errors=True)


if __name__ == '__main__':
    main()
