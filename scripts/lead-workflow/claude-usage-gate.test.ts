import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { ExtensionAPI, ExtensionContext } from '@oh-my-pi/pi-coding-agent';
import claudeUsageGate, { decide, probe, type Limit, type Probe } from './claude-usage-gate.ts';

const T0 = Date.parse('2026-10-26T18:00:00Z');
const MIN = 60_000;
const at = (ms: number) => new Date(ms).toISOString();
const session = (percent: number, reset = T0 + 60 * MIN): Limit => ({ label: 'Session (5-hour)', percent, resetsAt: at(reset) });
const weekly = (percent: number): Limit => ({ label: 'Weekly (7-day)', percent, resetsAt: at(T0 + 3 * 24 * 60 * MIN) });
const fable = (percent: number): Limit => ({ label: 'Fable Weekly', percent, resetsAt: at(T0 + 3 * 24 * 60 * MIN) });

function harness(steps: (Probe | Error)[], flags: Record<string, string> = {}) {
  let now = T0;
  const probes: boolean[] = [];
  const notes: string[] = [];
  let status: string | undefined;
  let handler: ((event: Record<string, unknown>, ctx: ExtensionContext) => Promise<unknown>) | undefined;
  const pi = {
    setLabel() {},
    registerFlag() {},
    getFlag: (name: string) => flags[name],
    on(name: string, fn: typeof handler) { if (name === 'before_provider_request') handler = fn; },
  } as unknown as ExtensionAPI; // test double: only the members the extension uses
  claudeUsageGate(pi, {
    now: () => now,
    async probe(force: boolean) {
      probes.push(force);
      const step = steps.shift();
      if (!step) throw new Error('unexpected probe');
      if (step instanceof Error) throw step;
      return step;
    },
  });
  async function request(model: { provider: string; id: string }) {
    const ui = { setStatus: (_k: string, text?: string) => { status = text; }, notify: (text: string) => { notes.push(text); } };
    const payload = { marker: 1 };
    const started = Date.now();
    const result = await handler!({ payload }, { model, ui } as unknown as ExtensionContext);
    assert.equal(result, payload, 'payload is returned unchanged');
    assert.ok(Date.now() - started < 1000, 'the monitor never holds a request');
  }
  return { request, probes, notes, status: () => status, advance: (ms: number) => { now += ms; } };
}

const opus = { provider: 'anthropic', id: 'claude-opus-5-5' };

test('non-Anthropic requests never query usage', async () => {
  const h = harness([]);
  await h.request({ provider: 'openai-codex', id: 'gpt-6-astra' });
  assert.deepEqual(h.probes, []);
});

test('below the limit clears the status and does not warn', async () => {
  const h = harness([{ limits: [session(0.5), weekly(0.3)], fetchedAt: T0 - MIN }]);
  await h.request(opus);
  assert.equal(h.status(), undefined);
  assert.deepEqual(h.notes, []);
});

test('a full window shows its reset time and warns once without holding the request', async () => {
  const h = harness([{ limits: [session(1, T0 + 90 * MIN), weekly(0.4)], fetchedAt: T0 - MIN }]);
  await h.request(opus);
  await h.request(opus); // cached observation, same state
  assert.match(h.status() ?? '', /Session \(5-hour\) 100% used; resets 2026-10-26 19:30Z/);
  assert.equal(h.notes.length, 1, 'one warning per state');
  assert.match(h.notes[0], /retry\.waitForUsageReset/);
  assert.deepEqual(h.probes, [false], 'usage is polled at most once a minute');
});

test('the threshold flag warns before 100%', async () => {
  const h = harness([{ limits: [session(0.9)], fetchedAt: T0 }], { 'claude-usage-threshold': '0.85' });
  await h.request(opus);
  assert.match(h.status() ?? '', /90% used/);
});

test('the Fable weekly window applies to Fable only', () => {
  const limits = [session(0.2), fable(1)];
  assert.equal(decide(limits, T0, T0, 'claude-opus-5-5', 0.98).kind, 'allow');
  assert.equal(decide(limits, T0, T0, 'claude-fable-5-1', 0.98).kind, 'wait');
});

test('a full window past its reset is still reported until a fresh fetch shows it below the limit', () => {
  const reset = T0 - 10 * MIN;
  const stale = decide([session(1, reset)], T0 - 20 * MIN, T0, 'claude-opus-5-5', 0.98);
  assert.equal(stale.kind, 'wait');
  const fresh = decide([session(0.02, T0 + 300 * MIN)], T0, T0, 'claude-opus-5-5', 0.98);
  assert.equal(fresh.kind, 'allow');
});

test('stale or unreadable usage is shown as unknown', async () => {
  const stale = harness([{ limits: [session(0.1)], fetchedAt: T0 - 60 * MIN }]);
  await stale.request(opus);
  assert.match(stale.status() ?? '', /unknown \(usage observation is stale\)/);
  const broken = harness([new Error('omarchy failed')]);
  await broken.request(opus);
  assert.match(broken.status() ?? '', /unknown \(usage command failed\)/);
});

test('probe reads limits from the command and freshness from the cache file', async t => {
  const dir = await mkdtemp(join(tmpdir(), 'usage-gate-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const cache = join(dir, 'claude-limits.json');
  await writeFile(cache, JSON.stringify({ fetchedAtMs: 1793039198609 }));
  const output = JSON.stringify({ updatedAt: '2026-10-26T18:26:42Z', limits: [session(0.16), { label: 'broken' }] });
  const forced: boolean[] = [];
  const result = await probe(true, async force => { forced.push(force); return output; }, cache);
  assert.deepEqual(forced, [true]);
  assert.equal(result.fetchedAt, 1793039198609, 'fetchedAtMs, not updatedAt');
  assert.deepEqual(result.limits, [session(0.16)], 'malformed limits are dropped');
  const missing = await probe(false, async () => output, join(dir, 'absent.json'));
  assert.equal(missing.fetchedAt, undefined);
});
