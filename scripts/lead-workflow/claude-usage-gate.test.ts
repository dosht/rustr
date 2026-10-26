import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { ExtensionAPI, ExtensionContext } from '@oh-my-pi/pi-coding-agent';
import claudeUsageGate, { probe, type Limit, type Probe } from './claude-usage-gate.ts';

const T0 = Date.parse('2026-10-26T18:00:00Z');
const MIN = 60_000;
const at = (ms: number) => new Date(ms).toISOString();
const session = (percent: number, reset = T0 + 60 * MIN): Limit => ({ label: 'Session (5-hour)', percent, resetsAt: at(reset) });
const weekly = (percent: number): Limit => ({ label: 'Weekly (7-day)', percent, resetsAt: at(T0 + 3 * 24 * 60 * MIN) });
const fable = (percent: number): Limit => ({ label: 'Fable Weekly', percent, resetsAt: at(T0 + 3 * 24 * 60 * MIN) });

type Step = Probe | Error;

function harness(steps: Step[], flags: Record<string, string> = {}) {
  let now = T0;
  const probes: boolean[] = [];
  const slept: number[] = [];
  const notes: string[] = [];
  let handler: ((event: Record<string, unknown>, ctx: ExtensionContext) => Promise<unknown>) | undefined;
  const pi = {
    setLabel() {},
    registerFlag() {},
    getFlag: (name: string) => flags[name],
    on(name: string, fn: typeof handler) { if (name === 'before_provider_request') handler = fn; },
  } as unknown as ExtensionAPI; // test double: only the members the extension uses
  claudeUsageGate(pi, {
    now: () => now,
    async sleep(ms: number, signal?: AbortSignal) { if (!signal?.aborted) { slept.push(ms); now += ms; } },
    async probe(force: boolean) {
      probes.push(force);
      const step = steps.shift();
      if (!step) throw new Error('unexpected probe');
      if (step instanceof Error) throw step;
      return step;
    },
  });
  async function request(model: { provider: string; id: string }, signal?: AbortSignal) {
    const ctx = { model, ui: { setStatus() {}, notify: (text: string) => { notes.push(text); } } } as unknown as ExtensionContext;
    const payload = { marker: 1 };
    const result = await handler!({ payload, ...(signal ? { signal } : {}) }, ctx);
    assert.equal(result, payload, 'payload is returned unchanged');
  }
  return { request, probes, slept, notes, advance: (ms: number) => { now += ms; }, now: () => now };
}

const opus = { provider: 'anthropic', id: 'claude-opus-5-5' };

test('non-Anthropic requests never query usage', async () => {
  const h = harness([]);
  await h.request({ provider: 'openai-codex', id: 'gpt-6-astra' });
  assert.deepEqual(h.probes, []);
});

test('below the limit with a fresh fetch passes immediately', async () => {
  const h = harness([{ limits: [session(0.5), weekly(0.3)], fetchedAt: T0 - MIN }]);
  await h.request(opus);
  assert.deepEqual(h.slept, []);
});

test('a full session window holds until its reset, then a forced fetch releases it', async () => {
  const reset = T0 + 90 * MIN;
  const h = harness([
    { limits: [session(1, reset), weekly(0.4)], fetchedAt: T0 - MIN },
    { limits: [session(0, reset + 5 * 60 * MIN), weekly(0.4)], fetchedAt: reset + MIN },
  ]);
  await h.request(opus);
  assert.equal(h.slept.reduce((a, b) => a + b, 0), 90 * MIN, 'slept exactly to the reset');
  assert.deepEqual(h.probes, [false, true], 'post-reset release needs a forced fetch');
  assert.match(h.notes.join('\n'), /Session \(5-hour\) 100% used; waiting until 2026-10-26 19:30Z/);
});

test('threshold flag holds before 100%', async () => {
  const h = harness([
    { limits: [session(0.9)], fetchedAt: T0 },
    { limits: [session(0.1)], fetchedAt: T0 + 61 * MIN },
  ], { 'claude-usage-threshold': '0.85' });
  await h.request(opus);
  assert.equal(h.slept.reduce((a, b) => a + b, 0), 60 * MIN);
});

test('the Fable weekly window gates Fable only', async () => {
  const h = harness([{ limits: [session(0.2), fable(1)], fetchedAt: T0 }]);
  await h.request(opus);
  assert.deepEqual(h.slept, [], 'Opus is not held by Fable usage');
  h.advance(2 * MIN); // past the poll cache
  const f = harness([{ limits: [session(0.2), fable(1)], fetchedAt: T0 }, { limits: [session(0.2), fable(0)], fetchedAt: T0 + 4 * 24 * 60 * MIN }]);
  await f.request({ provider: 'anthropic', id: 'claude-fable-5-1' });
  assert.ok(f.slept.length > 0, 'Fable is held');
});

test('stale or unreadable usage does not block an idle session', async () => {
  const stale = harness([{ limits: [session(0.1)], fetchedAt: T0 - 60 * MIN }]);
  await stale.request(opus);
  assert.deepEqual(stale.slept, []);
  assert.match(stale.notes.join('\n'), /stale; not blocking/);
  const broken = harness([new Error('omarchy failed')]);
  await broken.request(opus);
  assert.deepEqual(broken.slept, []);
});

test('an expired full window stays held while the post-reset fetch fails or is stale', async () => {
  const reset = T0 + 10 * MIN;
  const h = harness([
    { limits: [session(1, reset)], fetchedAt: T0 },
    new Error('fetch failed'), // forced fetch fails: cached full reading, past reset -> recheck
    { limits: [session(0)], fetchedAt: T0 - 120 * MIN }, // stale zero is not a confirmed refill
    { limits: [session(0, reset + 300 * MIN)], fetchedAt: reset + 11 * MIN },
  ]);
  await h.request(opus);
  assert.deepEqual(h.probes, [false, true, true, true]);
  assert.ok(h.now() >= reset + 10 * MIN, 'kept holding past the reset until a fresh fetch');
});

test('aborting the turn releases a hold', async () => {
  const controller = new AbortController();
  const h = harness([{ limits: [session(1, T0 + 300 * MIN)], fetchedAt: T0 }]);
  controller.abort();
  await h.request(opus, controller.signal);
  assert.deepEqual(h.slept, []);
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
