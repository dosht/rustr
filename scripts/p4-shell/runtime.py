#!/usr/bin/env python3
"""Owned disposable taskr runtime for the P4 browser and generated-Swift summary harnesses.

Starts, from explicit lead-built binary copies and nothing else:
  - a labeled postgres:17-alpine cluster (never pulled), owner DB + billing-provision runtime login,
  - scripts/p4-shell/provider.mjs, the held loopback completion server,
  - a loopback sentinel standing in for media storage, transcription and LemonSqueezy origins.
    The summary flow must never reach it; any request it records fails the run,
  - the actual taskr-server (dev auth, audio work, summary) and the actual Vite/wasm web shell,
then seeds one owned completed work for the Swift harness and prints the exact TASKR_P4_* env.

Usage (lead only, after coherent binaries and the wasm pkg are built):
  python3 scripts/p4-shell/runtime.py --bin-dir /tmp/<snapshot>/bin            # hold until Ctrl-C
  python3 scripts/p4-shell/runtime.py --bin-dir ... -- ./scripts/p4-shell/uniffi.sh
  python3 scripts/p4-shell/runtime.py --bin-dir ... -- sh -c 'cd apps/web && npx playwright test --config playwright.p4.config.ts'

No build, pull, production endpoint or real credential. Cleans only resources carrying this run's label.
Follows the proven P3 taskr smoke (p1Y): TCP readiness, provision order, restricted login, clean env.
"""

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parent.parent.parent
WEB = ROOT / 'apps/web'
PUBLIC = 'https://taskr-p4.invalid'
SUMMARY_KEY = 'p4-synthetic-key'  # must match provider.mjs
LABEL = 'rustr.p4.shell'


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def free_port():
    with socket.socket() as reserved:
        reserved.bind(('127.0.0.1', 0))
        return reserved.getsockname()[1]


OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def fetch(url, method='GET', token=None, timeout=5):
    headers = {'Authorization': 'Bearer ' + token} if token else {}
    request = urllib.request.Request(url, method=method, headers=headers)
    try:
        response = OPENER.open(request, timeout=timeout)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, response.read()


class Sentinel(http.server.BaseHTTPRequestHandler):
    """Records any request to the storage/transcriber/billing-provider origins and refuses it."""
    protocol_version = 'HTTP/1.1'

    def log_message(self, *args):
        pass

    def refuse(self):
        with self.server.lock:
            self.server.seen.append(f'{self.command} {self.path.split("?")[0]}')
        self.send_response(503)
        self.send_header('Content-Length', '0')
        self.send_header('Connection', 'close')
        self.end_headers()
        self.close_connection = True

    do_GET = do_HEAD = do_POST = do_PUT = do_DELETE = do_PATCH = refuse


class Runtime:
    def __init__(self, args):
        self.args = args
        self.bin = Path(args.bin_dir).resolve()
        self.run_id = uuid.uuid4().hex[:12]
        self.logs = Path(args.log_dir or f'/tmp/rustr-p4-shell-{self.run_id}').resolve()
        self.name = 'rustr-p4-shell-' + self.run_id
        self.db = 'p4_shell_' + self.run_id
        self.login = 'p4_shell_login_' + self.run_id
        self.password = 'local_' + uuid.uuid4().hex
        self.container = None
        self.children = []  # (label, Popen) in start order
        self.sentinel = None
        self.env_out = {}
        self.base_env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'), 'HOME': os.environ.get('HOME', '/tmp')}

    def command(self, argv, env=None, timeout=60):
        result = subprocess.run([str(a) for a in argv], cwd=ROOT, env=env or self.base_env, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout)
        require(result.returncode == 0, f'{Path(str(argv[0])).name} failed: {result.stdout.strip()}')
        return result.stdout.strip()

    def sql(self, statement, runtime=False):
        args = ['docker', 'exec']
        if runtime:
            args += ['-e', 'PGPASSWORD=' + self.password]
        args += [self.container, 'psql', '-X', '-A', '-t', '-v', 'ON_ERROR_STOP=1']
        if runtime:
            args += ['-h', '127.0.0.1']
        args += ['-U', self.login if runtime else 'postgres', '-d', self.db, '-c', statement]
        return self.command(args)

    def spawn(self, label, argv, env, cwd=ROOT):
        log = open(self.logs / f'{label}.log', 'w')
        process = subprocess.Popen([str(a) for a in argv], cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        self.children.append((label, process))
        return process

    def wait(self, predicate, label, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            for name, process in self.children:
                require(process.poll() is None, f'{name} exited early (see {self.logs}/{name}.log) while waiting for {label}')
            if predicate():
                return
            time.sleep(0.2)
        raise AssertionError('timed out: ' + label)

    def preflight(self):
        for binary in ('taskr-server', 'billing-provision'):
            require(os.access(self.bin / binary, os.X_OK), f'executable copy required: {self.bin / binary}')
        for tool in ('docker', 'node', 'psql'):
            require(any(os.access(Path(p) / tool, os.X_OK) for p in self.base_env['PATH'].split(':')), tool + ' is required')
        require((WEB / 'src/lib/core/pkg/taskr_wasm.js').exists(), 'actual wasm pkg missing: run TASKR_WASM_BUILD_STD=1 ./scripts/build-web.sh first')
        require((WEB / 'src/lib/core/types/AudioSummaryView.ts').exists(), 'generated summary types missing: run cargo test -p taskr-wasm first')
        require(os.access(WEB / 'node_modules/.bin/vite', os.X_OK), 'apps/web node_modules missing (npm ci)')
        self.command(['docker', 'image', 'inspect', self.args.postgres_image])
        self.logs.mkdir(parents=True, exist_ok=True)
        hashes = {b: hashlib.sha256((self.bin / b).read_bytes()).hexdigest() for b in ('taskr-server', 'billing-provision')}
        (self.logs / 'binaries.json').write_text(json.dumps(hashes, indent=2) + '\n')
        print('binaries: ' + ', '.join(f'{b} {h}' for b, h in hashes.items()), flush=True)

    def postgres(self):
        self.container = self.command(['docker', 'create', '--pull=never', '--name', self.name,
                                       '--label', f'{LABEL}={self.run_id}', '-e', 'POSTGRES_PASSWORD=' + self.password,
                                       '-p', '127.0.0.1::5432', self.args.postgres_image])
        self.command(['docker', 'start', self.container])
        ports = json.loads(self.command(['docker', 'inspect', '--format', '{{json .NetworkSettings.Ports}}', self.container]))
        port = int(ports['5432/tcp'][0]['HostPort'])
        deadline = time.monotonic() + 40
        # The image's init server listens on a Unix socket only, then restarts: TCP readiness is the real one.
        while subprocess.run(['docker', 'exec', self.container, 'pg_isready', '-h', '127.0.0.1', '-U', 'postgres'],
                             env=self.base_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5).returncode:
            require(time.monotonic() < deadline, 'owned PostgreSQL TCP readiness')
            time.sleep(0.2)
        self.command(['docker', 'exec', self.container, 'psql', '-X', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres',
                      '-d', 'postgres', '-c', f'CREATE DATABASE {self.db}'])
        self.owner = f'postgres://postgres:{self.password}@127.0.0.1:{port}/{self.db}'
        self.runtime_url = f'postgres://{self.login}:{self.password}@127.0.0.1:{port}/{self.db}'
        self.command([self.bin / 'billing-provision'], env={**self.base_env, 'BILLING_MIGRATION_DATABASE_URL': self.owner,
                     'BILLING_RUNTIME_LOGIN': self.login, 'BILLING_RUNTIME_PASSWORD': self.password})

    def fixtures(self):
        self.sentinel = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Sentinel)
        self.sentinel.daemon_threads = True
        self.sentinel.lock = threading.Lock()
        self.sentinel.seen = []
        threading.Thread(target=self.sentinel.serve_forever, daemon=True).start()
        self.sentinel_url = f'http://127.0.0.1:{self.sentinel.server_port}'
        provider_port = free_port()
        self.provider_url = f'http://127.0.0.1:{provider_port}'
        self.spawn('provider', ['node', ROOT / 'scripts/p4-shell/provider.mjs'],
                   {**self.base_env, 'TASKR_P4_PROVIDER_PORT': str(provider_port)})
        self.wait(lambda: 'P4_PROVIDER_READY' in (self.logs / 'provider.log').read_text(), 'loopback provider', 15)

    def host(self):
        port = free_port()
        self.api_root = f'http://127.0.0.1:{port}'
        env = {**self.base_env, 'RUST_BACKTRACE': '1', 'TASKR_BIND': f'127.0.0.1:{port}', 'TASKR_STORAGE': 'memory',
               'TASKR_DEV_AUTH': '1', 'TASKR_BILLING_DATABASE_URL': self.runtime_url,
               'TASKR_BILLING_MIGRATION_DATABASE_URL': self.owner, 'TASKR_PUBLIC_URL': PUBLIC,
               'TASKR_WORK_ENABLED': '1', 'TASKR_WORK_TICK_SECONDS': '1', 'TASKR_WORK_RETENTION_DAYS': '30',
               'MEDIA_R2_ENDPOINT': self.sentinel_url, 'MEDIA_R2_BUCKET': 'p4-unused',
               'MEDIA_R2_ACCESS_KEY': 'p4-unused', 'MEDIA_R2_SECRET_KEY': 'p4-unused-not-a-credential',
               'MEDIA_R2_REGION': 'auto', 'SONIOX_API_BASE_URL': self.sentinel_url,
               'SONIOX_API_KEY': 'p4-unused-not-a-credential', 'SONIOX_PROJECT_ID': 'p4-unused',
               'SONIOX_MODEL': 'stt-async-v5', 'LEMONSQUEEZY_API_URL': self.sentinel_url,
               'LEMONSQUEEZY_API_KEY': 'p4-unused', 'LEMONSQUEEZY_STORE_ID': '1', 'LEMONSQUEEZY_VARIANT_ID': '7',
               'LEMONSQUEEZY_WEBHOOK_SECRET': 'p4-unused', 'LEMONSQUEEZY_ENVIRONMENT': 'test',
               'TASKR_SUMMARY_ENABLED': '1', 'TASKR_SUMMARY_PROVIDER': 'deepseek',
               'TASKR_SUMMARY_API_KEY': SUMMARY_KEY, 'TASKR_SUMMARY_MODEL': 'p4-fixture',
               'TASKR_SUMMARY_BASE_URL': self.provider_url + '/v1'}
        self.spawn('taskr-server', [self.bin / 'taskr-server'], env)

        def ready():
            try:
                return fetch(self.api_root + '/api/billing/ready')[0] == 200
            except OSError:
                return False
        self.wait(ready, 'actual taskr readiness', 60)
        require(self.sql('SELECT current_user', runtime=True) == self.login, 'runtime login identity')
        require(self.sql("SELECT rolsuper OR rolcreaterole OR rolcreatedb OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
                         runtime=True) == 'f', 'runtime login must not hold elevated authority')

    def web(self):
        port = free_port()
        self.web_url = f'http://127.0.0.1:{port}'
        self.spawn('vite', [WEB / 'node_modules/.bin/vite', '--host', '127.0.0.1', '--port', port, '--strictPort'],
                   {**self.base_env, 'VITE_TASKR_API': '/api', 'TASKR_API_PROXY': self.api_root}, cwd=WEB)

        def ready():
            try:
                return fetch(self.web_url + '/')[0] == 200
            except OSError:
                return False
        self.wait(ready, 'actual Vite/wasm web shell', 60)

    def seed_native_owner(self):
        email = f'p4-swift-{self.run_id}@example.test'
        work = str(uuid.uuid4())
        env = {**self.base_env, 'TASKR_P4_API': self.api_root + '/api', 'TASKR_P4_EMAIL': email,
               'TASKR_P4_WORK_ID': work, 'TASKR_P4_FIXTURE_DATABASE_URL': self.owner}
        self.command(['node', ROOT / 'scripts/p4-shell/seed.mjs'], env=env)
        code, body = fetch(f'{self.api_root}/api/audio-work/{work}/summary', token='dev:' + email)
        require(code == 200 and json.loads(body) == {'summary': None, 'generation': None},
                f'summary routes must be mounted and empty before any start: {code} {body[:200]!r}')
        self.env_out = {'TASKR_P4_API': self.api_root + '/api', 'TASKR_P4_WEB_URL': self.web_url,
                        'TASKR_P4_PROVIDER_CONTROL': self.provider_url, 'TASKR_P4_FIXTURE_DATABASE_URL': self.owner,
                        'TASKR_P4_EMAIL': email, 'TASKR_P4_WORK_ID': work}
        lines = ''.join(f'export {k}={v}\n' for k, v in self.env_out.items())
        (self.logs / 'p4.env').write_text(lines)
        print('\nP4_RUNTIME_READY  (also in ' + str(self.logs / 'p4.env') + ')\n' + lines, flush=True)

    def hold(self, argv):
        if argv:
            result = subprocess.run(argv, cwd=ROOT, env={**os.environ, **self.env_out})
            return result.returncode
        stop = threading.Event()
        for sig in (signal.SIGINT, signal.SIGTERM):
            signal.signal(sig, lambda *_: stop.set())
        print('holding; Ctrl-C tears down only this run', flush=True)
        while not stop.wait(1):
            for name, process in self.children:
                require(process.poll() is None, f'{name} exited while holding (see {self.logs}/{name}.log)')
        return 0

    def teardown(self):
        failures = []
        # Reverse start order: Vite first closes proxied SSE attachments, which graceful host shutdown
        # would otherwise wait on; then the host (SIGINT, graceful); then the held provider.
        for name, process in reversed(self.children):
            if process.poll() is not None:
                continue
            try:
                os.killpg(process.pid, signal.SIGINT if name == 'taskr-server' else signal.SIGTERM)
                process.wait(timeout=15)
            except ProcessLookupError:
                continue
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=5)
                failures.append(f'{name} needed SIGKILL')
        if self.sentinel:
            self.sentinel.shutdown()
            with self.sentinel.lock:
                seen = list(self.sentinel.seen)
            (self.logs / 'sentinel.json').write_text(json.dumps(seen, indent=2) + '\n')
            if seen:
                failures.append('summary run reached storage/transcriber/billing-provider origins: ' + ', '.join(seen[:10]))
        if self.container:
            try:
                label = subprocess.run(['docker', 'inspect', '--format', '{{ index .Config.Labels "' + LABEL + '" }}', self.name],
                                       env=self.base_env, capture_output=True, text=True, timeout=30).stdout.strip()
                if label == self.run_id:
                    with open(self.logs / 'postgres.log', 'w') as log:
                        subprocess.run(['docker', 'logs', self.name], stdout=log, stderr=subprocess.STDOUT, timeout=30)
                    self.command(['docker', 'rm', '--force', '--volumes', self.name])
                else:
                    failures.append(f'refused to remove {self.name}: label {label!r} is not this run')
            except (OSError, subprocess.SubprocessError, AssertionError) as error:
                failures.append(f'cluster cleanup: {error}')
        return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin-dir', required=True, help='immutable copies of taskr-server and billing-provision')
    parser.add_argument('--postgres-image', default='postgres:17-alpine', help='already cached; never pulled')
    parser.add_argument('--log-dir', help='default /tmp/rustr-p4-shell-<run>')
    parser.add_argument('command', nargs=argparse.REMAINDER, help='optional: -- command to run with TASKR_P4_* set')
    args = parser.parse_args()
    argv = args.command[1:] if args.command[:1] == ['--'] else args.command
    runtime = Runtime(args)
    code = 1
    try:
        runtime.preflight()
        runtime.postgres()
        runtime.fixtures()
        runtime.host()
        runtime.web()
        runtime.seed_native_owner()
        code = runtime.hold(argv)
    except KeyboardInterrupt:
        print('P4_RUNTIME_INTERRUPTED', file=sys.stderr, flush=True)
        code = 130
    except (AssertionError, OSError, subprocess.SubprocessError) as error:
        print(f'P4_RUNTIME_FAILED: {error}', file=sys.stderr, flush=True)
    finally:
        failures = runtime.teardown()
    for failure in failures:
        print('P4_RUNTIME_CLEANUP: ' + failure, file=sys.stderr)
    print(f'logs: {runtime.logs}', flush=True)
    return code if not failures else (code or 1)


if __name__ == '__main__':
    sys.exit(main())
