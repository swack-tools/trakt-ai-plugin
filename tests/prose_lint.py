"""Warnings must fail the prose gate, even when Vale exits successfully."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    'check_prose', Path(__file__).resolve().parents[1] / 'scripts/check_prose.py'
)
prose = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prose)


class ProseLintTests(unittest.TestCase):
    def run_linter(self, output, exit_code=0):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / '--output.md').touch()
            result = subprocess.CompletedProcess([], exit_code, stdout=output, stderr='')
            with patch.object(prose, 'ROOT', root), \
                    patch.object(prose.subprocess, 'check_output', return_value=b'--output.md\0'), \
                    patch.object(prose.subprocess, 'run', return_value=result) as run:
                code = prose.main()
                argv = run.call_args.args[0]
                self.assertEqual(argv[-2:], ['--', str(root / '--output.md')])
                return code

    def test_warning_fails_even_with_successful_vale_exit(self):
        self.assertEqual(self.run_linter(
            '{"file.md":[{"Line":1,"Span":[1,4],"Check":"Google.WordList",'
            '"Message":"Use the preferred term","Severity":"warning"}]}'
        ), 1)

    def test_clean_prose_passes(self):
        self.assertEqual(self.run_linter('{}'), 0)

    def test_vale_failure_is_not_lost(self):
        self.assertEqual(self.run_linter('{}', 2), 2)


if __name__ == '__main__':
    unittest.main()
