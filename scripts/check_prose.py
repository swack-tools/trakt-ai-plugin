#!/usr/bin/env python3
"""Run Vale against tracked documentation and fail on warnings or errors."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    tracked = subprocess.check_output(
        ['git', 'ls-files', '-z', '--', '*.md', '*.html'], cwd=ROOT
    ).decode().split('\0')
    paths = [str(ROOT / name) for name in tracked if name and (ROOT / name).is_file()]
    if not paths:
        raise SystemExit('No tracked Markdown or HTML documentation found')
    # Absolute operands and -- prevent filenames from becoming Vale options.
    result = subprocess.run(
        ['vale', '--output=JSON', '--minAlertLevel=warning', '--', *paths],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    try:
        findings = json.loads(result.stdout)
    except json.JSONDecodeError:
        raise SystemExit(result.stderr or result.stdout or 'Vale did not return JSON') from None
    count = 0
    for filename, alerts in findings.items():
        for alert in alerts:
            print(f"{filename}:{alert['Line']}:{alert['Span'][0]}: "
                  f"{alert['Check']}: {alert['Message']}")
            count += 1
    print(f'Vale checked {len(paths)} documentation files: {count} warnings or errors')
    # Vale itself exits successfully for warnings; make the documented CI gate explicit.
    return 1 if count else result.returncode


if __name__ == '__main__':
    raise SystemExit(main())
