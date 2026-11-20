#!/usr/bin/env python3
"""Package the Linux tray as an AppImage and a .deb, with the engine, price tables and logos among the app's
resources (the .deb also installs the starmeter command), and the starmeter command on its own; smoke-test the
engine first. The release publishes them beside the Mac and Windows downloads."""
import argparse
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
    entries = {line.split()[-1]: line.split()[0] for line in listing if line.strip()}
    command = entries.get('./usr/bin/starmeter', '')
    if not command.startswith('-rwx') or './usr/lib/Starmeter/bin/starmeter-engine' not in entries:
        raise SystemExit('The .deb lacks /usr/bin/starmeter or the engine it runs:\n' + '\n'.join(listing))
    script = (ROOT / 'app/src-tauri/linux/starmeter').read_text(encoding='utf-8')
    if '/usr/lib/Starmeter/bin/starmeter-engine cli' not in script:
        raise SystemExit('/usr/bin/starmeter does not run the engine the .deb ships.')


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
        shutil.copy2(built[0], output / (name + suffix))
        print(output / (name + suffix))
    check_deb(output / (name + '.deb'))
    verify_command_line(command_line_archive(engine, output, name + '-cli.tar.gz'))
    shutil.rmtree(scratch, ignore_errors=True)


if __name__ == '__main__':
    main()
