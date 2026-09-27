#!/usr/bin/env python3
"""Build deterministic review artifacts; never publish or include operator files."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLUGIN = ROOT / 'plugins/trakt-mcp'


def archive(target, files):
    with zipfile.ZipFile(target, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for source, name in sorted(files, key=lambda item: item[1]):
            if source.is_symlink() or not source.is_file():
                raise ValueError(f'Only regular files can be packaged: {source}')
            info = zipfile.ZipInfo(name, date_time=(2020, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            z.writestr(info, source.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def package(output):
    from check_plugin import validate
    validate(PLUGIN)
    output = output.resolve()
    if output == PLUGIN or output.is_relative_to(PLUGIN):
        raise ValueError('Artifacts must be outside the installed plugin package')
    if output.exists() and any(output.iterdir()):
        raise ValueError('Output must be empty; choose a fresh directory to preserve existing artifacts')
    output.mkdir(parents=True, exist_ok=True)
    version = json.loads((PLUGIN / '.claude-plugin/plugin.json').read_text())['version']
    files = [(p, p.relative_to(PLUGIN).as_posix()) for p in PLUGIN.rglob('*') if p.is_file()]
    archive(output / f'trakt-mcp-{version}.zip', files)
    skill_files = [(p, p.relative_to(PLUGIN).as_posix()) for p in (PLUGIN/'skills').rglob('*') if p.is_file()]
    archive(output / f'trakt-mcp-skills-{version}.zip', skill_files + [(PLUGIN/'LICENSE', 'LICENSE')])
    for skill in sorted((PLUGIN/'skills').iterdir()):
        archive(output / f'{skill.name}-{version}.zip', [(p, p.relative_to(skill.parent).as_posix()) for p in skill.rglob('*') if p.is_file()] + [(PLUGIN/'LICENSE', skill.name + '/LICENSE')])
    # Revalidate the extracted plugin without access to the source checkout.
    with tempfile.TemporaryDirectory(prefix='trakt-plugin-package-') as tmp:
        with zipfile.ZipFile(output / f'trakt-mcp-{version}.zip') as z:
            z.extractall(tmp)
        validate(Path(tmp), repository_checks=False)
    checksums = ''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in sorted(output.glob('*.zip')))
    (output/'SHA256SUMS').write_text(checksums)
    print(f'Created {len(list(output.glob("*.zip")))} archives and SHA256SUMS in {output}')


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'dist/submission')
    package(parser.parse_args().output)
