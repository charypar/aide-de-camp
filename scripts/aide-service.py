"""Install and manage Aide's per-user service; never modify Tailscale or sudo."""
import argparse
import os
from pathlib import Path
import platform
import plistlib
import subprocess


ROOT = Path(__file__).resolve().parents[1]


class Service:
    def __init__(self, home, system, uid):
        self.home = home
        self.system = system
        if system == 'Darwin':
            self.path = home / 'Library/LaunchAgents/dev.aide.dashboard.plist'
            self.domain = f'gui/{uid}'
            self.target = f'{self.domain}/dev.aide.dashboard'
        elif system == 'Linux':
            self.path = home / '.config/systemd/user/aide.service'
            self.target = 'aide.service'
        else:
            raise RuntimeError('Service setup supports macOS and Linux only')

    def run(self, *args, probe=False):
        result = subprocess.run(args, capture_output=probe, check=not probe)
        return result.returncode == 0

    def install(self):
        binary = self.home / '.cargo/bin/aide'
        if not os.access(binary, os.X_OK):
            raise RuntimeError(f'Install Aide first with just install-aide (expected {binary})')

        if self.system == 'Darwin':
            template = plistlib.loads((ROOT / 'examples/dev.aide.dashboard.plist').read_bytes())
            template['ProgramArguments'][0] = str(binary)
            for key, value in template['EnvironmentVariables'].items():
                template['EnvironmentVariables'][key] = value.replace('/Users/YOU', str(self.home))
            for key in ('StandardOutPath', 'StandardErrorPath'):
                template[key] = template[key].replace('/Users/YOU', str(self.home))
            contents = plistlib.dumps(template)
            (self.home / 'Library/Logs').mkdir(parents=True, exist_ok=True)
        else:
            contents = (ROOT / 'examples/aide.service').read_bytes()

        # Existing service definitions may contain user-specific names, ports,
        # homes, or PATH entries. Never silently replace them during an upgrade.
        if not self.path.exists():
            self.path.parent.mkdir(parents=True, exist_ok=True)
            with self.path.open('xb') as output:
                output.write(contents)
            print(f'Installed {self.path}', flush=True)
        else:
            print(f'Preserving existing {self.path}', flush=True)

        if self.system == 'Darwin':
            self.run('launchctl', 'enable', self.target)
            if not self.run('launchctl', 'print', self.target, probe=True):
                self.run('launchctl', 'bootstrap', self.domain, str(self.path))
        else:
            self.run('systemctl', '--user', 'daemon-reload')
            self.run('systemctl', '--user', 'enable', '--now', self.target)

    def restart(self):
        if self.system == 'Darwin':
            if not self.run('launchctl', 'print', self.target, probe=True):
                raise RuntimeError('Service is not loaded; run just aide-service-install first')
            # SIGINT lets Aide close normally. KeepAlive restarts it with the new
            # binary without unloading the job or killing its task descendants.
            self.run('launchctl', 'kill', 'SIGINT', self.target)
        else:
            self.run('systemctl', '--user', 'restart', self.target)
        print('Restart requested; use just aide-service-status to inspect startup.', flush=True)

    def status(self):
        if self.system == 'Darwin':
            self.run('launchctl', 'print', self.target)
            print(f'Logs: {self.home}/Library/Logs/aide{{,-error}}.log')
        else:
            self.run('systemctl', '--user', 'status', '--no-pager', self.target)
            print('Logs: journalctl --user -u aide -f')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('install', 'restart', 'status'))
    args = parser.parse_args()
    try:
        service = Service(Path.home(), platform.system(), os.getuid())
        getattr(service, args.action)()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
