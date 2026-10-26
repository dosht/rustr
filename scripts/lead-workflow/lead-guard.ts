import type { ExtensionAPI, ExtensionContext, SessionEntry } from '@oh-my-pi/pi-coding-agent';
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { open } from 'node:fs/promises';
import { isAbsolute, relative, resolve } from 'node:path';

const STATE = 'rustr.lead-guard.observation.v1';
const REMINDER = 'rustr.lead-guard.reminder.v1';
const STATUS = 'rustr.lead-guard';
const MAX_FILE_BYTES = 1024 * 1024;
type RecordValue = Record<string, unknown>;
type Observation = {
  scope: string;
  session: string;
  material: string;
  revision: string;
  files: string[];
  warnings: string[];
};

function record(value: unknown): value is RecordValue {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function digest(value: unknown): string {
  return createHash('sha256').update(JSON.stringify(value)).digest('hex');
}

// Match the current OMP canonical todo sources, not rendered prose or tool inputs.
function todoSnapshot(entries: SessionEntry[]): unknown {
  for (let i = entries.length - 1; i >= 0; i--) {
    const entry = entries[i];
    let data: unknown;
    if (entry.type === 'custom' && entry.customType === 'user_todo_edit') data = entry.data;
    if (entry.type === 'message' && entry.message.role === 'toolResult' &&
        entry.message.toolName === 'todo' && !entry.message.isError) {
      data = entry.message.details;
      if (record(data) && data.op === 'view') continue;
    }
    if (!record(data) || !Array.isArray(data.phases)) continue;
    const phases = data.phases;
    if (!phases.every(phase => record(phase) && typeof phase.name === 'string' &&
      Array.isArray(phase.tasks) && phase.tasks.every(task => record(task) &&
        typeof task.content === 'string' &&
        ['pending', 'in_progress', 'completed', 'abandoned', 'blocked'].includes(String(task.status))))) continue;
    return phases.map(phase => ({
      name: phase.name,
      tasks: phase.tasks.map((task: RecordValue) => ({
        content: task.content, status: task.status, blocker: task.blocker,
      })),
    }));
  }
  return null;
}

function latestObservation(entries: SessionEntry[], observation: Observation): RecordValue | undefined {
  for (let i = entries.length - 1; i >= 0; i--) {
    const entry = entries[i];
    const data = entry.type === 'custom' && entry.customType === STATE ? entry.data :
      entry.type === 'custom_message' && entry.customType === REMINDER ? entry.details : undefined;
    if (record(data) && data.scope === observation.scope && data.session === observation.session &&
        typeof data.material === 'string' && typeof data.revision === 'string') return data;
  }
  return undefined;
}

async function fileRevision(root: string, name: string): Promise<{ revision: string; warning?: string }> {
  const path = resolve(root, name);
  const local = relative(root, path);
  if (isAbsolute(name) || local === '..' || local.startsWith('../')) {
    return { revision: 'outside-root', warning: `${name}: expected a repository-relative path` };
  }
  try {
    // Nonblocking open plus regular-file check also makes an accidental FIFO harmless.
    const file = await open(path, constants.O_RDONLY | constants.O_NONBLOCK);
    try {
      const stat = await file.stat();
      if (!stat.isFile() || stat.size > MAX_FILE_BYTES) {
        return { revision: 'unsupported-file', warning: `${name}: expected a regular file no larger than 1 MiB` };
      }
      const bytes = await file.readFile();
      return { revision: createHash('sha256').update(bytes).digest('hex') };
    } finally {
      await file.close();
    }
  } catch (error) {
    const code = record(error) && typeof error.code === 'string' ? error.code : 'unreadable';
    return { revision: code, warning: `${name}: ${code === 'ENOENT' ? 'missing' : `unreadable (${code})`}` };
  }
}

function reminder(observation: Observation): string {
  return [
    `Lead workflow reminder (observed revision ${observation.material.slice(0, 12)}; alignment unverified).`,
    `For material scope/progress changes, reconcile ${observation.files.join(', ')}, current decisions and current-batch OMP todos using skill://todo-state-machine before further work or handoff.`,
    'Check objective, decision revision, authority token, owner, evidence level and next action. Keep the full roadmap in WORKING-ON.md and only the current batch in session todos; preserve blockers and distinguish source-ready, executed evidence and acceptance.',
    'Do not automatically tick acceptance or rewrite unchanged status. A content hash is not proof of alignment. User pause/hold wins; this reminder authorizes no work or continuation. Ordinary conversation needs no bookkeeping edits.',
    ...observation.warnings.map(warning => `Warning: ${warning}.`),
  ].join('\n');
}

export default function leadGuard(pi: ExtensionAPI): void {
  pi.setLabel('Lead workflow reminder');
  pi.registerFlag('lead-guard-session', {
    type: 'string', description: 'Exact OMP session ID explicitly designated as lead (main sessions only)',
  });
  pi.registerFlag('lead-guard-decisions', {
    type: 'string', description: 'Comma-separated repository-relative current decision files; no timestamp inference',
  });

  // Factory-local: OMP rebinds factories to children but shares module globals.
  let generation = 0;
  let lastError = '';

  function designated(ctx: ExtensionContext): boolean {
    return ctx.agent.kind === 'main' && pi.getFlag('lead-guard-session') === ctx.sessionManager.getSessionId();
  }

  function inactive(ctx: ExtensionContext): void {
    if (ctx.agent.kind === 'main') ctx.ui.setStatus(STATUS, 'lead guard: inactive (session not designated)');
  }

  async function observe(ctx: ExtensionContext): Promise<Observation | undefined> {
    if (!designated(ctx)) { inactive(ctx); return; }
    const session = ctx.sessionManager.getSessionId();
    const root = ctx.cwd;
    const started = generation;
    const flag = pi.getFlag('lead-guard-decisions');
    const decisions = typeof flag === 'string' ? flag.split(',').map(path => path.trim()).filter(Boolean) : [];
    const files = [...new Set(['WORKING-ON.md', 'docs/plans/lead-preparation.md', ...decisions])].sort();
    const revisions = await Promise.all(files.map(file => fileRevision(root, file)));
    if (generation !== started || !designated(ctx) || ctx.cwd !== root || ctx.sessionManager.getSessionId() !== session) return;
    const entries = ctx.sessionManager.getBranch();
    const todos = todoSnapshot(entries);
    const warnings = revisions.flatMap(value => value.warning ? [value.warning] : []);
    if (!decisions.length) warnings.push('current decision files not designated (--lead-guard-decisions)');
    if (todos === null) warnings.push('no canonical current-batch todo snapshot on this branch');
    const scope = digest([root, files]);
    const material = digest([scope, revisions.map(value => value.revision), todos, warnings]);
    const context = entries.findLast(entry =>
      entry.type === 'compaction' || entry.type === 'branch_summary' || entry.type === 'reset_boundary')?.id;
    const observation = { scope, session, material, revision: digest([material, session, context]), files, warnings };
    ctx.ui.setStatus(STATUS, `lead guard: observed ${material.slice(0, 8)}, alignment unverified${warnings.length ? `; warnings ${warnings.length}` : ''}`);
    lastError = '';
    return observation;
  }

  async function safe<T>(ctx: ExtensionContext, action: () => Promise<T>): Promise<T | undefined> {
    const session = ctx.sessionManager.getSessionId();
    try {
      return await action();
    } catch (error) {
      if (!designated(ctx) || ctx.sessionManager.getSessionId() !== session) return;
      const message = error instanceof Error ? error.message : String(error);
      ctx.ui.setStatus(STATUS, 'lead guard: unavailable; alignment unverified');
      if (message !== lastError) ctx.ui.notify(`Lead guard unavailable: ${message}. Conversation remains available.`, 'warning');
      lastError = message;
      return;
    }
  }

  async function notice(ctx: ExtensionContext, reason: string, materialOnly = false): Promise<void> {
    await safe(ctx, async () => {
      const observation = await observe(ctx);
      if (!observation) return;
      const previous = latestObservation(ctx.sessionManager.getBranch(), observation);
      if (materialOnly ? previous?.material === observation.material : previous?.revision === observation.revision) return;
      ctx.ui.notify(`${reason}: ${reminder(observation)}`, observation.warnings.length ? 'warning' : 'info');
      pi.appendEntry(STATE, observation);
    });
  }

  // Lifecycle notifications never enqueue prompts. Rehydrate from the active branch each time.
  const boundary = async (_event: unknown, ctx: ExtensionContext) => {
    generation++;
    await notice(ctx, 'Session context');
  };
  pi.on('session_start', boundary);
  pi.on('session_switch', boundary);
  pi.on('session_branch', boundary);
  pi.on('session_tree', boundary);
  pi.on('session_before_compact', async (event, ctx) => {
    if (!event.signal.aborted) await notice(ctx, 'Before compaction');
  });
  pi.on('session_compact', async (_event, ctx) => {
    generation++;
    await notice(ctx, 'After compaction');
  });
  pi.on('auto_compaction_start', async (event, ctx) => {
    if (event.action === 'handoff') await notice(ctx, 'Handoff');
  });
  pi.on('session_before_switch', async (_event, ctx) => { await notice(ctx, 'Before session handoff', true); });
  pi.on('session_stop', async (event, ctx) => {
    if (!event.signal.aborted) await notice(ctx, 'Before yield', true);
    // Notification-only: no continue/block return, no prompt, no timers.
  });

  pi.on('before_agent_start', async (event, ctx) => safe(ctx, async () => {
    if (event.reason !== 'prompt' || (!event.prompt.trim() && !event.images?.length)) return;
    const observation = await observe(ctx);
    if (!observation) return;
    const entries = ctx.sessionManager.getBranch();
    const user = entries.findLast(entry => entry.type === 'message' && entry.message.role === 'user')?.id;
    const key = digest([observation.revision, user, event.prompt, event.images?.length ?? 0]);
    if (entries.some(entry => entry.type === 'custom_message' && entry.customType === REMINDER &&
      record(entry.details) && entry.details.key === key)) return;
    // No side effects here: OMP may discard/retry policy preparation. Only an accepted
    // returned message becomes a durable custom_message and a deduplication receipt.
    return { message: { customType: REMINDER, content: reminder(observation), display: false,
      details: { ...observation, key } } };
  }));

  pi.registerCommand('lead-guard', {
    description: 'Inspect passive lead reminder status; handoff checks material changes without starting a turn',
    handler: async (args, ctx) => {
      if (args.trim() === 'handoff') { await notice(ctx, 'Explicit handoff', true); return; }
      await safe(ctx, async () => {
        const observation = await observe(ctx);
        if (observation) ctx.ui.notify(reminder(observation), observation.warnings.length ? 'warning' : 'info');
      });
    },
  });
}
