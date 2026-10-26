#!/usr/bin/env python3
"""Owned disposable runtime for the P5 browser checkpoints (C2 flows, C3 container subset).

Status: source-ready. Run only on the lead's checkpoint authorization, with immutable binary copies.

Starts, from cached images and explicit binary copies only (never pulls, builds or installs):
  - a labeled docker network and postgres:17-alpine cluster: owner DB, billing-provision restricted
    runtime login, and GoTrue's own `auth` schema/role (deploy/dev/init-auth.sql shape);
  - a GoTrue container on that cluster (HS256 secret shared with the host, PKCE, SMTP to Mailpit,
    redirect allow-list = the app origin) behind an owned loopback `/auth/v1` prefix proxy;
  - a Mailpit container (GoTrue confirmation, magic-link and recovery mail; HTTP API for the specs);
  - the P4 loopback provider fixtures (R2 SigV4, Soniox, controlled DeepSeek, Lemon Squeezy, mail,
    PostHog, legacy Supabase bucket) plus a /__p5 control API so browser specs plan provider answers;
  - the actual transgate-server with the P4 jobs/billing/AI env and the real GoTrue directory.

  python3 scripts/p5-web/runtime.py --root /tmp/<snapshot> [--host-env KEY=VALUE ...]      # hold, Ctrl-C ends
  python3 scripts/p5-web/runtime.py --root /tmp/<snapshot> -- sh -c 'cd transgate/web && npx playwright test'

The command inherits P5_* variables (app/GoTrue/Mailpit/fixture URLs, admin emails, evidence dir,
PG container and login). Host static serving and runtime-config variables are passed explicitly with
--host-env once p1T publishes them; nothing is guessed here. Cleanup removes only resources carrying
this run's label. Local fixtures never establish live OAuth, captcha, payment, provider or SMTP acceptance.
"""

import argparse
import base64
import hashlib
import hmac
import http.client
import http.server
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

from iyzico_fixture import IyzicoFixture

REPO = Path(__file__).resolve().parents[2]
LABEL = 'rustr.p5.web'
JWT_SECRET = 'p5-local-jwt-secret-not-a-credential-0123456789'
GOTRUE_DB_PASSWORD = 'p5-local-auth-admin'
P4 = REPO / 'scripts/p4-ai/smoke-transgate.py'


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def load_p4():
    """The proven P4 loopback fixtures, imported read-only rather than copied."""
    spec = importlib.util.spec_from_file_location('p4_transgate_smoke', P4)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


p4 = load_p4()


def free_port():
    with socket.socket() as reserved:
        reserved.bind(('127.0.0.1', 0))
        return reserved.getsockname()[1]


def jwt(claims):
    b64 = lambda b: base64.urlsafe_b64encode(b).rstrip(b'=')
    head = b64(b'{"alg":"HS256","typ":"JWT"}')
    body = b64(json.dumps(claims, separators=(',', ':')).encode())
    signed = head + b'.' + body
    return (signed + b'.' + b64(hmac.new(JWT_SECRET.encode(), signed, hashlib.sha256).digest())).decode()


class P5Fixtures(p4.Fixtures):
    """P4 fixtures plus what an unscripted browser flow needs: provider plans set over HTTP, a
    default Soniox transcript for uploads, automatic Soniox callback delivery to the host, and the
    bucket CORS policy plus ranged GET a real R2 bucket gives a browser (signed PUT, media playback)."""

    def __init__(self, client):
        super().__init__(client)
        self.host_base = None
        self.app_origin = None
        self.soniox_default = None
        self.callback_delay = 0.5
        self.iyzico = IyzicoFixture()


class P5Handler(p4.FixtureHandler):
    def bucket_request(self):
        path = urllib.parse.urlsplit(self.path).path
        return path.count('/') >= 2 and path.split('/')[1] in self.fixture.buckets.values()

    def end_headers(self):
        # The bucket CORS rule: only the app origin, only on bucket objects (never the provider APIs).
        origin = self.headers.get('Origin')
        if origin and origin == self.fixture.app_origin and self.bucket_request():
            self.send_header('Access-Control-Allow-Origin', origin)
            self.send_header('Access-Control-Expose-Headers', 'ETag, Content-Length, Content-Range, Accept-Ranges')
            self.send_header('Vary', 'Origin')
        super().end_headers()

    def do_OPTIONS(self):
        origin = self.headers.get('Origin')
        method = self.headers.get('Access-Control-Request-Method', '')
        if not (self.bucket_request() and origin == self.fixture.app_origin and method in ('GET', 'PUT', 'HEAD')):
            self.fixture.fail(f'unexpected preflight {method} {self.path.split("?")[0]} from {origin}')
            return self.reply(403)
        self.send_response(204)
        self.send_header('Access-Control-Allow-Methods', 'GET, PUT, HEAD')
        self.send_header('Access-Control-Allow-Headers', self.headers.get('Access-Control-Request-Headers', ''))
        self.send_header('Access-Control-Max-Age', '600')
        self.send_header('Content-Length', '0')
        self.send_header('Connection', 'close')
        self.end_headers()
        self.close_connection = True

    def handle_owned(self):
        path = urllib.parse.urlsplit(self.path).path
        if path.startswith('/__p5/'):
            return self.control(self.fixture, path)
        if self.fixture.iyzico.handles(self.command, path):
            with self.fixture.lock:
                return self.fixture.iyzico.handle(self, path, self.fixture.app_origin, p4.PUBLIC)
        if self.command == 'POST' and path == '/v1/transcriptions':
            return self.soniox_submit(self.fixture)
        if self.command == 'GET' and self.headers.get('Range') and self.bucket_request():
            return self.ranged(self.fixture, path)
        return super().handle_owned()

    def ranged(self, f, path):
        """Single `bytes=a-b` / `bytes=a-` range on a signed GET, as the media element asks for."""
        if not f.signature_valid('GET', self.path, self.headers['Host']):
            return self.reply(403, b'<Error><Code>SignatureDoesNotMatch</Code></Error>', 'application/xml')
        with f.lock:
            f.events.append((time.time(), 'GET-RANGE', urllib.parse.unquote(path)))
            held = f.objects.get(urllib.parse.unquote(path))
        if held is None:
            return self.reply(404, b'')
        unit, _, spec = self.headers['Range'].partition('=')
        start_text, _, end_text = spec.partition('-')
        require(unit == 'bytes' and ',' not in spec and start_text.isdigit(), 'unsupported Range ' + self.headers['Range'])
        start = int(start_text)
        end = min(int(end_text), len(held) - 1) if end_text else len(held) - 1
        if start >= len(held) or start > end:
            self.send_response(416)
            self.send_header('Content-Range', f'bytes */{len(held)}')
            self.send_header('Content-Length', '0')
            self.send_header('Connection', 'close')
            self.end_headers()
            self.close_connection = True
            return
        part = held[start:end + 1]
        self.send_response(206)
        self.send_header('Content-Type', 'application/octet-stream')
        self.send_header('Accept-Ranges', 'bytes')
        self.send_header('Content-Range', f'bytes {start}-{end}/{len(held)}')
        self.send_header('Content-Length', str(len(part)))
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(part)
        self.close_connection = True

    def soniox_submit(self, f):
        """Default plan for browser uploads, then the real P4 Soniox handler, then the callback."""
        raw = self.body()
        body = json.loads(raw)
        job = int(urllib.parse.parse_qs(urllib.parse.urlsplit(body['webhook_url']).query)['job_id'][0])
        with f.lock:
            if job not in f.soniox_plans:
                require(f.soniox_default is not None, f'unplanned Soniox job {job}: set /__p5/soniox-default or a per-job plan')
                f.soniox_plans[job] = dict(f.soniox_default)
            hold = f.soniox_plans[job].get('hold', False)
        # Re-feed the consumed body to the P4 handler.
        self.rfile = _Replay(raw, self.rfile)
        self.headers.replace_header('Content-Length', str(len(raw)))
        super().handle_owned()
        if not hold:
            threading.Thread(target=deliver_callback, args=(f, job), daemon=True).start()

    def control(self, f, path):
        if self.command == 'GET' and path == '/__p5/calls':
            with f.lock:
                calls = [{k: c[k] for k in ('marker', 'mode', 'rid', 'client_gone', 'finished', 'at')} for c in f.llm_calls]
                return self.reply(200, {'llm': calls, 'mail': [{k: m[k] for k in ('to', 'subject', 'key', 'at')} for m in f.mail],
                                        'failures': f.failures, 'analytics': [e.get('event') for e in f.analytics]})
        body = json.loads(self.body() or b'{}')
        if self.command == 'POST' and path == '/__p5/iyzico-plan':
            with f.lock:
                f.iyzico.plan(body)
            return self.reply(200, {'ok': True})
        if self.command == 'POST' and path == '/__p5/llm-plan':
            # {"marker": str, "mode": "analysis"|"chat"|"summary", "steps": [["delta","..."],["hold","name"],["finish","stop"]]}
            steps = []
            for step in body['steps']:
                if step[0] == 'hold':
                    event = threading.Event()
                    with f.lock:
                        f.holds = getattr(f, 'holds', {})
                        f.holds[step[1]] = event
                    steps.append(('hold', event))
                else:
                    steps.append(tuple(step))
            f.plan(body['marker'], body['mode'], p4.Plan(*steps))
            return self.reply(200, {'ok': True})
        if self.command == 'POST' and path.startswith('/__p5/release/'):
            name = path.rsplit('/', 1)[-1]
            with f.lock:
                event = getattr(f, 'holds', {}).get(name)
            require(event is not None, 'unknown hold ' + name)
            event.set()
            return self.reply(200, {'ok': True})
        if self.command == 'POST' and path == '/__p5/soniox-default':
            # {"text": str, "duration_ms": int}
            with f.lock:
                f.soniox_default = {'tokens': [{'text': body['text'], 'start_ms': 0, 'end_ms': 1000, 'speaker': '1'}],
                                    'duration_ms': int(body.get('duration_ms', 60000))}
            return self.reply(200, {'ok': True})
        raise AssertionError('unknown control request ' + path)


class _Replay:
    def __init__(self, data, rest):
        self.data, self.rest = data, rest

    def read(self, n=-1):
        if self.data:
            chunk, self.data = (self.data, b'') if n < 0 or n >= len(self.data) else (self.data[:n], self.data[n:])
            return chunk
        return self.rest.read(n)

    def readline(self, *args):
        return self.rest.readline(*args)


def deliver_callback(f, job):
    """Soniox's own webhook, delivered over the P4 HTTPS-origin mapping to the real host listener."""
    time.sleep(f.callback_delay)
    record = f.record(job)
    body = json.dumps({'id': record['remote'], 'status': 'completed', 'audio_duration_ms': record['duration_ms']}).encode()
    status, _ = f.client.call(f.host_base + record['path'], 'POST', body, headers={'Content-Type': 'application/json', **record['headers']})
    if status != 200:
        f.fail(f'automatic Soniox callback for job {job} answered {status}')


class AuthPrefixProxy(http.server.BaseHTTPRequestHandler):
    """`/auth/v1/*` -> GoTrue root, as Supabase's gateway does. Streams bodies and keeps headers."""
    protocol_version = 'HTTP/1.1'

    def log_message(self, *args):
        pass

    def forward(self):
        path = self.path
        if not path.startswith('/auth/v1/') and path != '/auth/v1':
            self.send_response(404)
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        upstream = path[len('/auth/v1'):] or '/'
        length = int(self.headers.get('Content-Length') or 0)
        body = self.rfile.read(length) if length else None
        headers = {k: v for k, v in self.headers.items() if k.lower() not in ('host', 'connection', 'content-length')}
        conn = http.client.HTTPConnection('127.0.0.1', self.server.gotrue_port, timeout=30)
        try:
            conn.request(self.command, upstream, body=body, headers=headers)
            response = conn.getresponse()
            data = response.read()
            self.send_response(response.status)
            for name, value in response.getheaders():
                if name.lower() not in ('connection', 'transfer-encoding', 'content-length'):
                    self.send_header(name, value)
            self.send_header('Content-Length', str(len(data)))
            self.send_header('Connection', 'close')
            self.end_headers()
            if self.command != 'HEAD':
                self.wfile.write(data)
        finally:
            conn.close()
        self.close_connection = True

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = do_HEAD = forward


class Runtime:
    def __init__(self, args):
        self.args = args
        self.root = Path(args.root).resolve()
        self.run = uuid.uuid4().hex
        self.short = self.run[:12]
        self.dir = Path(args.log_dir or f'/tmp/rustr-p5-web-runtime-{self.short}')
        self.dir.mkdir(parents=True, exist_ok=False)
        self.network = f'rustr-p5-net-{self.short}'
        self.pg = f'rustr-p5-pg-{self.short}'
        self.gotrue = f'rustr-p5-gotrue-{self.short}'
        self.mailpit = f'rustr-p5-mailpit-{self.short}'
        self.db = f'p5_web_{self.short}'
        self.login = f'p5_web_login_{self.short}'
        self.password = 'local_' + uuid.uuid4().hex
        self.created = []
        self.network_created = False
        self.servers = []
        self.process = None
        self.logs = []
        self.clean_env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'), 'RUST_BACKTRACE': '1'}
        self.client = p4.LocalHttp()
        self.fixture = P5Fixtures(self.client)
        self.admins = [f'admin-{self.short}@example.test', f'admin2-{self.short}@example.test']

    # ---- docker -------------------------------------------------------------------

    def docker(self, *args, timeout=60, check=True):
        result = subprocess.run(['docker', *args], env=self.clean_env, capture_output=True, text=True, timeout=timeout)
        require(not check or result.returncode == 0, f'docker {args[0]} failed: {result.stderr.strip()}')
        return result.stdout.strip()

    def run_container(self, name, image, *args):
        self.docker('image', 'inspect', image)
        self.docker('run', '-d', '--pull=never', '--name', name, '--label', f'{LABEL}={self.run}', '--network', self.network, *args, image)
        self.created.append(name)

    def port(self, name, inner):
        return int(json.loads(self.docker('inspect', '--format', '{{json .NetworkSettings.Ports}}', name))[f'{inner}/tcp'][0]['HostPort'])

    def sql(self, statement, database=None):
        return self.docker('exec', self.pg, 'psql', '-X', '-A', '-t', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1',
                           '-U', 'postgres', '-d', database or self.db, '-c', statement)

    def wait(self, predicate, label, seconds=60):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return
            except (OSError, urllib.error.URLError, AssertionError, subprocess.SubprocessError):
                pass
            if self.process:
                require(self.process.poll() is None, 'transgate-server exited while waiting for ' + label)
            time.sleep(0.3)
        raise AssertionError('timed out: ' + label)

    # ---- startup ------------------------------------------------------------------

    def start(self):
        for binary in ('transgate-server', 'billing-provision'):
            require(os.access(self.root / 'target/debug' / binary, os.X_OK), 'immutable binary copy required: ' + binary)
        self.docker('network', 'create', '--label', f'{LABEL}={self.run}', self.network)
        self.network_created = True
        self.run_container(self.pg, self.args.postgres_image, '-e', 'POSTGRES_PASSWORD=' + self.password, '-p', '127.0.0.1::5432')
        pg_port = self.port(self.pg, 5432)
        self.wait(lambda: self.sql('SELECT 1', 'postgres') == '1', 'owned PostgreSQL over TCP')
        self.sql(f'CREATE DATABASE {self.db}', 'postgres')
        # GoTrue's prerequisites, exactly the dev stack's init-auth.sql, on this run's database only.
        for statement in (
            f"CREATE USER supabase_auth_admin NOINHERIT CREATEROLE LOGIN PASSWORD '{GOTRUE_DB_PASSWORD}'",
            'CREATE SCHEMA IF NOT EXISTS auth AUTHORIZATION supabase_auth_admin',
            f'GRANT CREATE ON DATABASE {self.db} TO supabase_auth_admin',
            "ALTER USER supabase_auth_admin SET search_path = 'auth'",
        ):
            self.sql(statement)
        owner = f'postgres://postgres:{self.password}@127.0.0.1:{pg_port}/{self.db}'
        runtime = f'postgres://{self.login}:{self.password}@127.0.0.1:{pg_port}/{self.db}'
        subprocess.run([self.root / 'target/debug/billing-provision'], cwd=self.root, check=True, timeout=120,
                       env={**self.clean_env, 'BILLING_MIGRATION_DATABASE_URL': owner, 'BILLING_RUNTIME_LOGIN': self.login,
                            'BILLING_RUNTIME_PASSWORD': self.password})

        # Fixtures first: the host and GoTrue both need their URLs.
        fixture = http.server.ThreadingHTTPServer(('127.0.0.1', 0), P5Handler)
        fixture.daemon_threads = True
        fixture.fixture = self.fixture
        self.fixture_url = f'http://127.0.0.1:{fixture.server_port}'
        self.fixture.base = self.fixture_url
        self.client.origins.add(self.fixture_url)
        self.fixture.buckets = {'upload': 'uploads-' + self.short[:8], 'archive': 'archive-' + self.short[:8],
                                'staging': 'staging-' + self.short[:8]}
        self.serve(fixture)

        host_port = free_port()
        self.app_url = f'http://127.0.0.1:{host_port}'
        self.client.origins.add(self.app_url)
        self.fixture.host_base = self.app_url
        self.fixture.app_origin = self.app_url

        self.run_container(self.mailpit, self.args.mailpit_image, '-p', '127.0.0.1::8025')
        self.mailpit_url = f'http://127.0.0.1:{self.port(self.mailpit, 8025)}'

        proxy = http.server.ThreadingHTTPServer(('127.0.0.1', 0), AuthPrefixProxy)
        proxy.daemon_threads = True
        self.auth_url = f'http://127.0.0.1:{proxy.server_port}/auth/v1'
        self.run_container(self.gotrue, self.args.gotrue_image, '-p', '127.0.0.1::9999', *self.gotrue_env())
        proxy.gotrue_port = self.port(self.gotrue, 9999)
        self.serve(proxy)
        self.wait(lambda: self.client.call(self.auth_url + '/health')[0] == 200, 'GoTrue health through the prefix proxy')

        service_key = jwt({'role': 'service_role', 'iss': 'p5-local', 'iat': int(time.time()), 'exp': int(time.time()) + 86400})
        self.env = {
            **self.clean_env, 'TRANSGATE_BIND': f'127.0.0.1:{host_port}',
            'TRANSGATE_DATABASE_URL': runtime, 'TRANSGATE_MIGRATION_DATABASE_URL': owner, 'TRANSGATE_PUBLIC_URL': p4.PUBLIC,
            'SUPABASE_JWT_SECRET': JWT_SECRET, 'SUPABASE_URL': self.auth_url[:-len('/auth/v1')], 'SUPABASE_SERVICE_ROLE_KEY': service_key,
            'TRANSGATE_ADMIN_EMAILS': ','.join(self.admins), 'TRANSGATE_SIGNUP_TOKEN': 'p5-signup-token',
            'TRANSGATE_MAIL_URL': self.fixture_url + '/emails', 'TRANSGATE_MAIL_API_KEY': p4.MAIL_KEY,
            'TRANSGATE_MAIL_FROM': 'billing@example.test', 'TRANSGATE_SALES_EMAIL': 'sales@example.test',
            'TRANSGATE_SUPPORT_EMAIL': 'support@example.test', 'POSTHOG_HOST': self.fixture_url, 'POSTHOG_API_KEY': p4.POSTHOG_KEY,
            'LEMONSQUEEZY_API_URL': self.fixture_url, 'LEMONSQUEEZY_API_KEY': p4.LEMON_KEY, 'LEMONSQUEEZY_STORE_ID': '1',
            'LEMONSQUEEZY_VARIANT_ID': '7', 'LEMONSQUEEZY_WEBHOOK_SECRET': p4.LEMON_SECRET, 'LEMONSQUEEZY_ENVIRONMENT': 'test',
            'IYZIPAY_URI': self.fixture_url, 'IYZIPAY_API_KEY': 'p5-iyzico-unused', 'IYZIPAY_SECRET_KEY': 'p5-iyzico-unused',
            'IYZIPAY_MERCHANT_ID': 'p5-merchant', 'IYZIPAY_ENVIRONMENT': 'sandbox', 'USD_TRY_RATE': '44',
            'TRANSGATE_JOBS_ENABLED': '1', 'TRANSGATE_JOBS_TICK_SECONDS': '1', 'CRON_SECRET': p4.CRON_SECRET,
            'STORAGE_PROVIDER': 'cloudflare', 'MEDIA_R2_ENDPOINT': self.fixture_url, 'MEDIA_R2_BUCKET': self.fixture.buckets['upload'],
            'MEDIA_R2_ACCESS_KEY': p4.ACCESS, 'MEDIA_R2_SECRET_KEY': p4.STORAGE_SECRET, 'MEDIA_R2_REGION': 'auto',
            'TRANSGATE_ARCHIVE_BUCKET': self.fixture.buckets['archive'], 'TRANSGATE_STAGING_BUCKET': self.fixture.buckets['staging'],
            'SONIOX_API_BASE_URL': self.fixture_url, 'SONIOX_API_KEY': p4.SONIOX_KEY, 'SONIOX_PROJECT_ID': p4.SONIOX_PROJECT,
            'SONIOX_MAX_DURATION_MINUTES': '200',
            'TRANSCRIPT_API_BASE_URL': self.fixture_url, 'TRANSCRIPT_API_KEY': 'p5-transcript-unused',
            'YOUTUBE_METADATA_BASE_URL': self.fixture_url, 'YOUTUBE_API_KEY': 'p5-youtube-unused', 'YOUTUBE_PUBLIC_BASE_URL': self.fixture_url,
            'LEGACY_SUPABASE_URL': self.fixture_url, 'LEGACY_SUPABASE_SERVICE_ROLE_KEY': p4.LEGACY_KEY,
            'ENABLE_AI_FEATURES': 'true', 'ENABLE_AI_CHAT': 'true', 'DEEPSEEK_API_KEY': p4.LLM_KEY, 'DEEPSEEK_BASE_URL': self.fixture_url,
            'AI_CHAT_MAX_QUESTIONS_PER_TRANSCRIPT': '2',
        }
        if self.args.web_dir:
            # p1T's published static/public-config env (hosts.md). apiUrl is fixed same-origin /api;
            # telemetry stays unset so no PostHog/Sentry config reaches the shell unless a spec owns it.
            web_dir = Path(self.args.web_dir).resolve()
            require((web_dir / 'index.html').is_file(), 'built web dir needs index.html: ' + str(web_dir))
            self.env.update({'TRANSGATE_WEB_DIR': str(web_dir), 'TRANSGATE_WEB_SITE_URL': self.app_url,
                             'TRANSGATE_WEB_AUTH_URL': self.auth_url})
        for item in self.args.host_env:
            key, sep, value = item.partition('=')
            require(sep and key, 'host env must be KEY=VALUE: ' + item)
            self.env[key] = value
        log = open(self.dir / 'transgate.log', 'w+')
        self.logs.append(log)
        self.process = subprocess.Popen([self.root / 'target/debug/transgate-server'], cwd=self.root, env=self.env,
                                        stdout=log, stderr=log, start_new_session=True)
        self.wait(lambda: self.client.call(self.app_url + '/api/billing/ready')[0] == 200, 'transgate-server readiness')

    def gotrue_env(self):
        site = self.app_url
        pairs = {
            'GOTRUE_API_HOST': '0.0.0.0', 'PORT': '9999', 'API_EXTERNAL_URL': self.auth_url,
            'GOTRUE_SITE_URL': site, 'GOTRUE_URI_ALLOW_LIST': f'{site},{site}/**',
            'GOTRUE_DB_DRIVER': 'postgres',
            'GOTRUE_DB_DATABASE_URL': f'postgres://supabase_auth_admin:{GOTRUE_DB_PASSWORD}@{self.pg}:5432/{self.db}?search_path=auth',
            'GOTRUE_JWT_SECRET': JWT_SECRET, 'GOTRUE_JWT_AUD': 'authenticated', 'GOTRUE_JWT_EXP': '3600',
            'GOTRUE_JWT_DEFAULT_GROUP_NAME': 'authenticated', 'GOTRUE_DISABLE_SIGNUP': 'false',
            'GOTRUE_EXTERNAL_EMAIL_ENABLED': 'true', 'GOTRUE_MAILER_AUTOCONFIRM': 'false',
            # GoTrue resolves absolute mailer paths against the origin, not API_EXTERNAL_URL's
            # prefix. Match the mounted proxy without rewriting delivered links.
            'GOTRUE_MAILER_URLPATHS_CONFIRMATION': '/auth/v1/verify',
            'GOTRUE_MAILER_URLPATHS_INVITE': '/auth/v1/verify',
            'GOTRUE_MAILER_URLPATHS_RECOVERY': '/auth/v1/verify',
            'GOTRUE_MAILER_URLPATHS_EMAIL_CHANGE': '/auth/v1/verify',
            'GOTRUE_SMTP_HOST': self.mailpit, 'GOTRUE_SMTP_PORT': '1025', 'GOTRUE_SMTP_ADMIN_EMAIL': 'auth@example.test',
            'GOTRUE_SMTP_SENDER_NAME': 'Transgate P5 local', 'GOTRUE_RATE_LIMIT_EMAIL_SENT': '1000',
            # Loopback suite mails one address repeatedly; production/staging keep real throttles (P9).
            'GOTRUE_SMTP_MAX_FREQUENCY': '1s',
            # Google is enabled with synthetic credentials: /authorize must redirect with the PKCE challenge;
            # the Google leg itself is never contacted (deployed acceptance stays a named live gate).
            'GOTRUE_EXTERNAL_GOOGLE_ENABLED': 'true', 'GOTRUE_EXTERNAL_GOOGLE_CLIENT_ID': 'p5-synthetic-google-client',
            'GOTRUE_EXTERNAL_GOOGLE_SECRET': 'p5-synthetic-google-secret',
            'GOTRUE_EXTERNAL_GOOGLE_REDIRECT_URI': self.auth_url + '/callback',
        }
        out = []
        for key, value in pairs.items():
            out += ['-e', f'{key}={value}']
        return out

    def serve(self, server):
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.servers.append(server)
        if server.server_address[0] == '127.0.0.1':
            self.client.origins.add(f'http://127.0.0.1:{server.server_port}')

    def exported(self):
        return {
            'P5_APP_URL': self.app_url, 'P5_AUTH_URL': self.auth_url, 'P5_MAILPIT_URL': self.mailpit_url,
            'P5_FIXTURE_URL': self.fixture_url, 'P5_ADMIN_EMAILS': ','.join(self.admins), 'P5_EVIDENCE_DIR': str(self.dir),
            'P5_PG_CONTAINER': self.pg, 'P5_PG_DATABASE': self.db, 'P5_RUN': self.run,
        }

    # ---- teardown -----------------------------------------------------------------

    def stop(self):
        failures = []
        if self.process and self.process.poll() is None:
            self.process.send_signal(signal.SIGINT)
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                failures.append('transgate-server ignored SIGINT for 20 s (graceful shutdown waits for open SSE bodies)')
                self.process.kill()
                self.process.wait(timeout=5)
        for server in self.servers:
            server.shutdown()
            server.server_close()
        trace = {'failures': self.fixture.failures, 'events': self.fixture.events,
                 'llm_calls': [{k: c[k] for k in ('marker', 'mode', 'rid', 'client_gone', 'finished', 'at')} for c in self.fixture.llm_calls]}
        (self.dir / 'fixture-trace.json').write_text(json.dumps(trace, indent=1))
        (self.dir / 'iyzico-trace.json').write_text(json.dumps({
            'calls': self.fixture.iyzico.calls,
            'unconsumed_plans': list(self.fixture.iyzico.plans),
            'checkout_count': len(self.fixture.iyzico.checkouts),
        }, indent=2))
        for name in reversed(self.created):
            found = subprocess.run(['docker', 'inspect', name], env=self.clean_env, capture_output=True, text=True, timeout=15)
            if found.returncode != 0:
                continue
            owned = json.loads(found.stdout)[0]
            if owned['Config']['Labels'].get(LABEL) != self.run:
                failures.append('refusing to remove non-owned container ' + name)
                continue
            (self.dir / f'{name}.log').write_text(self.docker('logs', '--tail', '300', name, check=False))
            self.docker('rm', '-f', '-v', owned['Id'], check=False)
        if self.network_created:
            labels = json.loads(self.docker('network', 'inspect', self.network))[0]['Labels'] or {}
            if labels.get(LABEL) == self.run:
                self.docker('network', 'rm', self.network, check=False)
            else:
                failures.append('refusing to remove non-owned network ' + self.network)
        for log in self.logs:
            log.close()
        require(not failures, 'cleanup: ' + '; '.join(failures))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--root', required=True, help='immutable snapshot root holding target/debug binaries')
    parser.add_argument('--postgres-image', default='postgres:17-alpine')
    parser.add_argument('--gotrue-image', default='public.ecr.aws/supabase/gotrue:v2.197.0')
    parser.add_argument('--mailpit-image', default='public.ecr.aws/supabase/mailpit:v1.31.3')
    parser.add_argument('--log-dir', help='new evidence directory')
    parser.add_argument('--web-dir', help='immutable copy of the built transgate/web dist served by the host')
    parser.add_argument('--host-env', action='append', default=[], help='extra transgate-server KEY=VALUE (p1T-published only)')
    parser.add_argument('command', nargs=argparse.REMAINDER, help='-- command to run with P5_* env, else hold until Ctrl-C')
    args = parser.parse_args()
    require(shutil.which('docker'), 'docker is required')
    command = args.command[1:] if args.command[:1] == ['--'] else args.command

    def interrupted(signum, frame):
        raise KeyboardInterrupt(f'signal {signum}')
    signal.signal(signal.SIGTERM, interrupted)
    runtime = Runtime(args)
    code = 1
    try:
        runtime.start()
        env = runtime.exported()
        for key, value in env.items():
            print(f'{key}={value}')
        sys.stdout.flush()
        if command:
            code = subprocess.run(command, cwd=REPO, env={**os.environ, **env}).returncode
        else:
            print('runtime held; Ctrl-C ends and cleans up', file=sys.stderr)
            while True:
                time.sleep(3600)
    except KeyboardInterrupt:
        code = 130 if command else 0
    finally:
        runtime.stop()
        print(f'EVIDENCE {runtime.dir}', file=sys.stderr)
    sys.exit(code)


if __name__ == '__main__':
    main()
