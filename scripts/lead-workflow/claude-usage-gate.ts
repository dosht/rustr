import type { ExtensionAPI, ExtensionContext } from '@oh-my-pi/pi-coding-agent';
import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';

// Holds Anthropic provider requests while an observed Claude usage window is at its limit,
// then sleeps until that window resets. Usage comes from `omarchy agent usage claude`.
// Freshness is the limits cache's `fetchedAtMs`, not the record's `updatedAt`: an expired
// window that the collector reports as empty is not proof of a refill until a fetch newer
// than the reset confirms it. Unknown or stale usage never blocks (fail open, with a warning).

export interface Limit { label: string; percent: number; resetsAt: string }
export type Decision =
  | { kind: 'allow' }
  | { kind: 'unknown'; reason: string }
  | { kind: 'wait'; label: string; percent: number; until: number };

const STATUS = 'claude-usage-gate';
const CACHE = join(homedir(), '.cache/omarchy/agent-usage/claude-limits.json');
const POLL_MS = 60_000;
const STALE_MS = 15 * 60_000;
const RECHECK_MS = 5 * 60_000;
const SLICE_MS = 30_000;

/** Which windows constrain a model: the shared session and weekly windows always, Fable's own weekly window only for Fable. */
export function applies(label: string, modelId: string): boolean {
  if (/fable/i.test(label)) return /fable/i.test(modelId);
  return /session|weekly|5-hour|7-day/i.test(label);
}

/** Pure decision from one observation. `fetchedAt` is the successful-fetch time in ms. */
export function decide(limits: Limit[], fetchedAt: number | undefined, now: number, modelId: string, threshold: number): Decision {
  if (fetchedAt === undefined) return { kind: 'unknown', reason: 'no successful fetch time' };
  let wait: Decision = { kind: 'allow' };
  for (const limit of limits) {
    if (!applies(limit.label, modelId)) continue;
    const reset = Date.parse(limit.resetsAt);
    if (!Number.isFinite(reset) || limit.percent < threshold) continue;
    // A full window past its reset time stays held until a fresh fetch reports it below the limit.
    const until = reset > now ? reset : now + RECHECK_MS;
    if (wait.kind !== 'wait' || until > wait.until) wait = { kind: 'wait', label: limit.label, percent: limit.percent, until };
  }
  if (wait.kind === 'allow' && now - fetchedAt > STALE_MS) return { kind: 'unknown', reason: 'usage observation is stale' };
  return wait;
}

export interface Probe { limits: Limit[]; fetchedAt: number | undefined }

function run(force: boolean): Promise<string> {
  const args = ['agent', 'usage', 'claude', '--limits-only', ...(force ? ['--force'] : [])];
  const { promise, resolve, reject } = Promise.withResolvers<string>();
  execFile('omarchy', args, { timeout: 60_000 }, (error, stdout) => (error ? reject(error) : resolve(stdout)));
  return promise;
}

function isLimit(value: unknown): value is Limit {
  return !!value && typeof value === 'object' && 'label' in value && typeof value.label === 'string'
    && 'percent' in value && typeof value.percent === 'number'
    && 'resetsAt' in value && typeof value.resetsAt === 'string';
}

export async function probe(force: boolean, exec = run, cache = CACHE): Promise<Probe> {
  const record: unknown = JSON.parse(await exec(force));
  const limits = record && typeof record === 'object' && 'limits' in record && Array.isArray(record.limits)
    ? record.limits.filter(isLimit) : [];
  let fetchedAt: number | undefined;
  try {
    const raw: unknown = JSON.parse(await readFile(cache, 'utf8'));
    if (raw && typeof raw === 'object' && 'fetchedAtMs' in raw && typeof raw.fetchedAtMs === 'number') fetchedAt = raw.fetchedAtMs;
  } catch { /* missing cache: freshness unknown */ }
  return { limits, fetchedAt };
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  if (signal?.aborted) return Promise.resolve();
  const done = () => { clearTimeout(timer); signal?.removeEventListener('abort', done); resolve(); };
  const timer = setTimeout(done, ms);
  signal?.addEventListener('abort', done, { once: true });
  return promise;
}

export default function claudeUsageGate(pi: ExtensionAPI, deps = { probe, now: () => Date.now(), sleep }): void {
  pi.setLabel('Claude usage gate');
  pi.registerFlag('claude-usage-threshold', {
    type: 'string', description: 'Consumed fraction (0-1] at which Anthropic requests wait for the window reset; default 0.98',
  });

  let last: { at: number; result: Probe } | undefined;
  let warned = '';

  async function observe(force: boolean): Promise<Probe | undefined> {
    const now = deps.now();
    if (!force && last && now - last.at < POLL_MS) return last.result;
    try {
      last = { at: now, result: await deps.probe(force) };
      return last.result;
    } catch {
      return last?.result;
    }
  }

  pi.on('before_provider_request', async (event, ctx: ExtensionContext) => {
    const model = ctx.model;
    if (!model || model.provider !== 'anthropic') return event.payload;
    const flag = Number(pi.getFlag('claude-usage-threshold') ?? '0.98');
    const threshold = Number.isFinite(flag) && flag > 0 && flag <= 1 ? flag : 0.98;

    const signal = 'signal' in event && event.signal instanceof AbortSignal ? event.signal : undefined;
    let force = false;
    for (;;) {
      if (signal?.aborted) break;
      const observation = await observe(force);
      const decision = observation
        ? decide(observation.limits, observation.fetchedAt, deps.now(), model.id, threshold)
        : { kind: 'unknown' as const, reason: 'usage command failed' };
      if (decision.kind === 'allow') { ctx.ui.setStatus(STATUS, undefined); break; }
      if (decision.kind === 'unknown') {
        if (force) {
          // Already holding: an unreadable usage record after a reset is not a confirmed refill.
          ctx.ui.setStatus(STATUS, `claude usage gate: reset unconfirmed (${decision.reason}); rechecking`);
          await deps.sleep(RECHECK_MS, signal);
          continue;
        }
        if (warned !== decision.reason) ctx.ui.notify(`Claude usage gate: ${decision.reason}; not blocking.`, 'warning');
        warned = decision.reason;
        ctx.ui.setStatus(STATUS, `claude usage: unknown (${decision.reason})`);
        break;
      }
      warned = '';
      const until = new Date(decision.until).toISOString().slice(0, 16).replace('T', ' ') + 'Z';
      const text = `${decision.label} ${Math.round(decision.percent * 100)}% used; waiting until ${until}`;
      ctx.ui.setStatus(STATUS, `claude usage gate: ${text}`);
      ctx.ui.notify(`Claude usage gate: ${text}.${signal ? ' Esc cancels the turn.' : ''}`, 'warning');
      while (!signal?.aborted && deps.now() < decision.until) await deps.sleep(Math.min(SLICE_MS, decision.until - deps.now()), signal);
      force = true; // after a reset, only a fresh fetch may release the hold
    }
    return event.payload;
  });
}
