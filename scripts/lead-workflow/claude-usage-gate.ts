import type { ExtensionAPI, ExtensionContext } from '@oh-my-pi/pi-coding-agent';
import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';

// Claude usage monitor. Before each Anthropic request it reads `omarchy agent usage claude`
// (at most once a minute) and shows the most constraining window in the status line, warning
// once when a window reaches the threshold. It does not hold requests: OMP gives extension
// handlers a 30-second budget, so blocking until a reset is done by the built-in
// `retry.waitForUsageReset: true` (set in lead-session.yml and worker-session.yml), which waits
// for the provider's authoritative reset time and stays abortable.
// Freshness is the limits cache's `fetchedAtMs`, not the record's `updatedAt`.

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

export default function claudeUsageGate(pi: ExtensionAPI, deps = { probe, now: () => Date.now() }): void {
  pi.setLabel('Claude usage gate');
  pi.registerFlag('claude-usage-threshold', {
    type: 'string', description: 'Consumed fraction (0-1] at which the Claude usage monitor warns; default 0.98',
  });

  let last: { at: number; result: Probe } | undefined;
  let warned = '';

  async function observe(): Promise<Probe | undefined> {
    const now = deps.now();
    if (last && now - last.at < POLL_MS) return last.result;
    try {
      last = { at: now, result: await deps.probe(false) };
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

    const observation = await observe();
    const decision = observation
      ? decide(observation.limits, observation.fetchedAt, deps.now(), model.id, threshold)
      : { kind: 'unknown' as const, reason: 'usage command failed' };
    if (decision.kind === 'allow') {
      ctx.ui.setStatus(STATUS, undefined);
      warned = '';
    } else if (decision.kind === 'unknown') {
      ctx.ui.setStatus(STATUS, `claude usage: unknown (${decision.reason})`);
      if (warned !== decision.reason) ctx.ui.notify(`Claude usage monitor: ${decision.reason}.`, 'warning');
      warned = decision.reason;
    } else {
      const until = new Date(decision.until).toISOString().slice(0, 16).replace('T', ' ') + 'Z';
      const text = `${decision.label} ${Math.round(decision.percent * 100)}% used; resets ${until}`;
      ctx.ui.setStatus(STATUS, `claude usage: ${text}`);
      if (warned !== text) ctx.ui.notify(`Claude usage: ${text}. Requests wait for the reset (retry.waitForUsageReset).`, 'warning');
      warned = text;
    }
    return event.payload;
  });
}
