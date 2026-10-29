import argparse
import base64
import hashlib
import http.server
import json
import secrets
import threading
import urllib.error
import urllib.request


class AuthProxy(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, status, body):
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def handle_request(self):
        raw = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        if self.path == '/__android/pkce' and self.command == 'POST':
            body = json.loads(raw)
            credentials = json.dumps({k: body[k] for k in ('email', 'password')}).encode()
            status, session = self.forward(self.server.auth + '/token?grant_type=password', credentials)
            if status != 200:
                return self.reply(status, session)
            code = secrets.token_urlsafe(24)
            with self.server.lock:
                self.server.codes[code] = {'challenge': body['challenge'], 'session': session, 'exchanges': 0}
            return self.reply(200, json.dumps({'code': code}).encode())
        if self.path.startswith('/__android/pkce/') and self.command == 'GET':
            with self.server.lock:
                entry = self.server.codes.get(self.path.rsplit('/', 1)[-1])
                body = {'exchanges': entry['exchanges']} if entry else {'error': 'unknown code'}
            return self.reply(200 if entry else 404, json.dumps(body).encode())
        if self.path == '/auth/v1/token?grant_type=pkce' and self.command == 'POST':
            body = json.loads(raw)
            challenge = base64.urlsafe_b64encode(hashlib.sha256(body['code_verifier'].encode()).digest()).rstrip(b'=').decode()
            with self.server.lock:
                entry = self.server.codes.get(body['auth_code'])
                if entry and entry['challenge'] == challenge and entry['exchanges'] == 0:
                    entry['exchanges'] += 1
                    return self.reply(200, entry['session'])
            return self.reply(400, b'{"error":"invalid_grant","error_description":"Invalid PKCE code or verifier"}')
        origin = self.server.auth + self.path[len('/auth/v1'):] if self.path.startswith('/auth/v1/') else self.server.app + self.path
        status, body = self.forward(origin, raw)
        self.reply(status, body)

    def forward(self, url, body):
        headers = {key: value for key, value in self.headers.items() if key.lower() not in ('host', 'content-length', 'connection')}
        request = urllib.request.Request(url, data=body if self.command != 'GET' else None, headers=headers, method=self.command)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status, response.read()
        except urllib.error.HTTPError as error:
            return error.code, error.read()

    do_GET = handle_request
    do_POST = handle_request
    do_PATCH = handle_request
    do_PUT = handle_request
    do_DELETE = handle_request


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--auth', required=True)
    parser.add_argument('--port', type=int, default=44921)
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', args.port), AuthProxy)
    server.app, server.auth = args.app.rstrip('/'), args.auth.rstrip('/')
    server.codes, server.lock = {}, threading.Lock()
    server.serve_forever()
