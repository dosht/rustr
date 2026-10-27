"""Owned C2/C3 protocol fixture, not an Iyzico sandbox or issuer emulator.

Wire contracts: modules/billing/gateways/iyzico/src/{client,sign}.rs;
D17/D19: docs/plans/p5-web/implementation/billing.md sections 5-6.
Every initialization requires an explicit queued plan. No live network calls.
"""
import base64
import hashlib
import hmac
import html
import json
import time
import urllib.parse
import uuid
from collections import deque

PAYG_INIT = '/payment/iyzipos/checkoutform/initialize/auth/ecom'
PAYG_RESULT = '/payment/iyzipos/checkoutform/auth/ecom/detail'
SUB_INIT = '/v2/subscription/checkoutform/initialize'
SUB_RESULT = '/v2/subscription/checkoutform/'
SUB_DETAIL = '/v2/subscription/subscriptions/'
API_KEY = 'p5-iyzico-unused'
SECRET = 'p5-iyzico-unused'


class IyzicoFixture:
    def __init__(self):
        self.plans = deque()
        self.checkouts = {}
        self.subscriptions = {}
        self.calls = []

    def plan(self, body):
        outcomes = ('success', 'failure', 'unconfirmed', 'init_rejected')
        if set(body) != {'kind', 'outcome'} or body['kind'] not in ('payg', 'subscription') or body['outcome'] not in outcomes:
            raise AssertionError('Iyzico plan requires kind payg|subscription and outcome ' + '|'.join(outcomes))
        self.plans.append(dict(body))

    def handles(self, method, path):
        return ((method == 'POST' and path in (PAYG_INIT, PAYG_RESULT, SUB_INIT))
                or (method == 'GET' and path.startswith(SUB_RESULT))
                or (method in ('GET', 'POST') and path.startswith(SUB_DETAIL)))

    def handle(self, handler, path, app_origin, public_origin):
        raw = handler.body() if handler.command == 'POST' else b''
        nonce = handler.headers.get('x-iyzi-rnd', '')
        if not nonce:
            raise AssertionError('missing Iyzico request nonce')
        signed = raw if handler.command == 'POST' else b'{}'
        digest = hmac.new(SECRET.encode(), nonce.encode() + path.encode() + signed, hashlib.sha256).hexdigest()
        expected = 'IYZWSv2 ' + base64.b64encode(f'apiKey:{API_KEY}&randomKey:{nonce}&signature:{digest}'.encode()).decode()
        if not hmac.compare_digest(handler.headers.get('Authorization', ''), expected):
            raise AssertionError('Iyzico signature does not bind exact request bytes/path/nonce')
        body = json.loads(raw) if raw else None
        self.calls.append({'at': time.time(), 'method': handler.command, 'path': path})
        if path in (PAYG_INIT, SUB_INIT):
            kind = 'payg' if path == PAYG_INIT else 'subscription'
            if not self.plans or self.plans[0]['kind'] != kind:
                raise AssertionError('unplanned Iyzico initialization ' + kind)
            plan = self.plans.popleft()
            callback = urllib.parse.urlsplit(body['callbackUrl'])
            if f'{callback.scheme}://{callback.netloc}' not in (app_origin, public_origin):
                raise AssertionError('checkout callback is not bound to the owned runtime')
            if callback.path != '/checkout/confirm' or callback.fragment:
                raise AssertionError('unexpected checkout callback route')
            if not body.get('conversationId') or body.get('locale') not in ('en', 'tr'):
                raise AssertionError('missing checkout reference/provider locale')
            if kind == 'payg':
                if body['price'] != body['paidPrice'] or body['basketItems'][0]['price'] != body['price']:
                    raise AssertionError('checkout amount/basket mismatch')
                if body['currency'] not in ('USD', 'TRY', 'EUR'):
                    raise AssertionError('unexpected checkout currency')
            elif not body.get('pricingPlanReferenceCode'):
                raise AssertionError('subscription needs published pricing plan')
            if plan['outcome'] == 'init_rejected':
                # Documented failure envelope: the adapter reports Rejected and no form is rendered.
                return handler.reply(200, {'status': 'failure', 'errorCode': 'p5_init_rejected',
                                           'errorMessage': 'Fixture rejected checkout initialization'})
            token = 'p5-' + uuid.uuid4().hex
            record = {**plan, 'request': body, 'token': token, 'payment_id': 'p5-payment-' + uuid.uuid4().hex,
                      'subscription': 'p5-sub-' + uuid.uuid4().hex, 'customer': 'p5-customer-' + uuid.uuid4().hex}
            self.checkouts[token] = record
            # Map only the configured synthetic public origin onto the owned loopback host.
            # Path/query are preserved; this is fixture delivery, not a spec rewriting a real link.
            action = app_origin + callback.path + ('?' + callback.query if callback.query else '')
            form = ('<form method="post" action="' + html.escape(action, quote=True) + '">'
                    '<input type="hidden" name="token" value="' + token + '">'
                    '<button type="submit">Complete fixture payment</button></form>'
                    '<script>document.documentElement.dataset.iyzicoFixture="ready";</script>')
            answer = {'token': token, 'checkoutFormContent': form}
            return handler.reply(200, {'status': 'success', **({'data': answer} if kind == 'subscription' else answer)})
        if path == PAYG_RESULT:
            if set(body) != {'locale', 'conversationId', 'token'} or body['locale'] != 'en' or body['conversationId'] != '123456789':
                raise AssertionError('unexpected PAYG result request')
            record = self.checkouts[body['token']]
            if record['kind'] != 'payg':
                raise AssertionError('wrong checkout kind')
            state = {'success': 'SUCCESS', 'failure': 'FAILURE', 'unconfirmed': 'PENDING'}[record['outcome']]
            return handler.reply(200, {'status': 'success', 'token': record['token'], 'paymentStatus': state,
                                      'paidPrice': record['request']['paidPrice'], 'currency': record['request']['currency'],
                                      'paymentId': record['payment_id']})
        if handler.command == 'GET' and path.startswith(SUB_RESULT):
            record = self.checkouts[urllib.parse.unquote(path[len(SUB_RESULT):])]
            if record['kind'] != 'subscription':
                raise AssertionError('wrong subscription checkout kind')
            if record['outcome'] != 'success':
                # Adapter intentionally classifies a rejected lookup as uncertain, not a decline.
                return handler.reply(200, {'status': 'failure', 'errorCode': 'fixture_pending', 'errorMessage': 'Not confirmed'})
            detail = {'referenceCode': record['subscription'], 'customerReferenceCode': record['customer'],
                      'pricingPlanReferenceCode': record['request']['pricingPlanReferenceCode'],
                      'subscriptionStatus': 'ACTIVE', 'orders': []}
            self.subscriptions[record['subscription']] = detail
            return handler.reply(200, {'status': 'success', 'data': detail})
        suffix = path[len(SUB_DETAIL):]
        if handler.command == 'POST' and suffix.endswith('/cancel'):
            ref = urllib.parse.unquote(suffix[:-len('/cancel')])
            if body != {} or ref not in self.subscriptions:
                raise AssertionError('unbound subscription cancellation')
            self.subscriptions[ref]['subscriptionStatus'] = 'CANCELED'
            return handler.reply(200, {'status': 'success'})
        if handler.command == 'GET' and suffix and '/' not in suffix:
            return handler.reply(200, {'status': 'success', 'data': self.subscriptions[urllib.parse.unquote(suffix)]})
        raise AssertionError('unsupported Iyzico protocol route')
