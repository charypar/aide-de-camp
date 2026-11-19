"""Service lifecycle contracts without touching the user's service manager."""
import importlib.util
from pathlib import Path
import plistlib
import os
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('aide_service', Path(__file__).parents[1] / 'scripts/aide-service.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name) / 'Name With Spaces & Symbols'
        self.home.mkdir()
        binary = self.home / '.cargo/bin/aide'
        binary.parent.mkdir(parents=True)
        binary.touch(mode=0o755)

    def test_mac_install_renders_paths_and_preserves_customizations(self):
        service = module.Service(self.home, 'Darwin', 501)
        with patch.object(service, 'run', return_value=True) as run:
            service.install()
            data = plistlib.loads(service.path.read_bytes())
            self.assertEqual(data['ProgramArguments'][0], str(self.home / '.cargo/bin/aide'))
            self.assertIn('49173', data['ProgramArguments'])
            self.assertEqual(data['StandardErrorPath'], str(self.home / 'Library/Logs/aide-error.log'))
            data['EnvironmentVariables']['AIDE_NAME'] = 'Custom'
            service.path.write_bytes(plistlib.dumps(data))
            service.install()
            self.assertEqual(plistlib.loads(service.path.read_bytes())['EnvironmentVariables']['AIDE_NAME'], 'Custom')
            self.assertFalse(any('bootout' in call.args for call in run.call_args_list))

    def test_mac_restart_signals_only_dashboard_and_requires_loaded_service(self):
        service = module.Service(self.home, 'Darwin', 501)
        with patch.object(service, 'run', return_value=False) as run:
            with self.assertRaises(RuntimeError):
                service.restart()
            self.assertEqual(run.call_count, 1)
        with patch.object(service, 'run', return_value=True) as run:
            service.restart()
            self.assertEqual(run.call_args.args, ('launchctl', 'kill', 'SIGINT', 'gui/501/dev.aide.dashboard'))

    def test_linux_install_enables_and_preserves_process_lifetime(self):
        service = module.Service(self.home, 'Linux', 1000)
        with patch.object(service, 'run', return_value=True) as run:
            service.install()
            self.assertIn('KillMode=process', service.path.read_text())
            self.assertIn('"%h/.cargo/bin/aide"', service.path.read_text())
            self.assertEqual(run.call_args.args, ('systemctl', '--user', 'enable', '--now', 'aide.service'))

    @unittest.skipUnless(shutil.which('just'), 'Just is required for upgrade ordering test')
    def test_failed_install_does_not_request_restart(self):
        bin_dir = Path(self.temp.name) / 'bin'
        bin_dir.mkdir()
        for name, body in [('trunk', 'exit 0'), ('cargo', 'exit 19'),
                           ('python3', 'echo UNEXPECTED_SERVICE_RESTART; exit 0')]:
            command = bin_dir / name
            command.write_text('#!/bin/sh\n' + body + '\n')
            command.chmod(0o755)
        result = subprocess.run(
            ['just', '--justfile', str(module.ROOT / 'justfile'), 'upgrade-aide'],
            env={**os.environ, 'PATH': str(bin_dir) + os.pathsep + os.environ['PATH']},
            capture_output=True, text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('UNEXPECTED_SERVICE_RESTART', result.stdout)

    def test_install_without_binary_does_not_write_or_start(self):
        (self.home / '.cargo/bin/aide').unlink()
        service = module.Service(self.home, 'Darwin', 501)
        with patch.object(service, 'run') as run:
            with self.assertRaises(RuntimeError):
                service.install()
            run.assert_not_called()
            self.assertFalse(service.path.exists())


if __name__ == '__main__':
    unittest.main()
