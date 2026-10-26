import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm, stat, utimes } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { ExtensionAPI, ExtensionContext, SessionEntry } from '@oh-my-pi/pi-coding-agent';
import leadGuard from './lead-guard.ts';

type RawEntry = Record<string, unknown>;
type Handler = (event: Record<string, unknown>, ctx: ExtensionContext) => unknown | Promise<unknown>;
type Prepared = { message: { customType: string; content: string; display: boolean; details: Record<string, unknown> } };
const phases = (status = 'in_progress', blocker?: string) => [{ name: 'PREP-1', tasks: [{
  content: 'Prepare only; product remains held', status, ...(blocker ? { blocker } : {}),
}] }];

async function fixture(t: test.TestContext) {
  const root = await mkdtemp(join(tmpdir(), 'lead-guard-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(join(root, 'docs/plans'), { recursive: true });
  await writeFile(join(root, 'WORKING-ON.md'), '# PREP-A: product PAUSED\n');
  await writeFile(join(root, 'docs/plans/lead-preparation.md'), '# PREP-1 only; no product release\n');
  await writeFile(join(root, 'docs/plans/decisions.md'), '# Decision 1: preserve the hold\n');
  const flags = new Map<string, string>([
    ['lead-guard-session', 'lead-session'],
    ['lead-guard-decisions', 'docs/plans/decisions.md'],
  ]);
  let entries: RawEntry[] = [{ id: 'todo-initial', type: 'custom', customType: 'user_todo_edit', data: { phases: phases() } }];
  let session = 'lead-session';
  let kind = 'main';
  let failPersistence = false;
  let serial = 0;
  let handlers = new Map<string, Handler>();
  let commands = new Map<string, (args: string, ctx: ExtensionContext) => Promise<void>>();
  const notices: { text: string; level: string }[] = [];
  const statuses: string[] = [];
  const forbidden: string[] = [];
  const forbid = (name: string) => () => { forbidden.push(name); throw new Error(`Forbidden action: ${name}`); };
  const context = {
    cwd: root,
    get agent() { return { kind, id: kind === 'sub' ? 'advisor' : session, name: kind, depth: 0 }; },
    sessionManager: {
      getSessionId: () => session,
      getBranch: () => structuredClone(entries) as unknown as SessionEntry[],
    },
    ui: {
      setStatus: (_key: string, text: string) => { statuses.push(text); },
      notify: (text: string, level: string) => { notices.push({ text, level }); },
    },
    abort: forbid('abort'), compact: forbid('compact'), shutdown: forbid('shutdown'),
    setInterval: forbid('setInterval'), setTimeout: forbid('setTimeout'),
    runEphemeralTurn: forbid('runEphemeralTurn'),
  } as unknown as ExtensionContext;

  function bind() {
    handlers = new Map();
    commands = new Map();
    const api = {
      setLabel: () => {},
      registerFlag: () => {},
      getFlag: (name: string) => flags.get(name),
      on: (name: string, handler: Handler) => { handlers.set(name, handler); },
      registerCommand: (name: string, value: { handler: (args: string, ctx: ExtensionContext) => Promise<void> }) => {
        commands.set(name, value.handler);
      },
      appendEntry: (customType: string, data: unknown) => {
        if (failPersistence) throw new Error('synthetic session storage failure');
        entries.push({ id: `guard-${++serial}`, type: 'custom', customType, data: structuredClone(data) });
      },
      sendMessage: forbid('sendMessage'), sendUserMessage: forbid('sendUserMessage'),
      exec: forbid('exec'), setModel: forbid('setModel'), registerTool: forbid('registerTool'),
    } as unknown as ExtensionAPI;
    leadGuard(api);
  }
  bind();
  const fire = async (name: string, payload: Record<string, unknown> = {}) => {
    const handler = handlers.get(name);
    assert.ok(handler, `Missing handler for ${name}`);
    return await handler({ type: name, signal: new AbortController().signal, ...payload }, context);
  };
  return {
    root, flags, notices, statuses, forbidden, fire, bind,
    get entries() { return structuredClone(entries); },
    set entries(value: RawEntry[]) { entries = structuredClone(value); },
    set session(value: string) { session = value; },
    set kind(value: string) { kind = value; },
    set failPersistence(value: boolean) { failPersistence = value; },
    push(entry: RawEntry) { entries.push(structuredClone(entry)); },
    async prepare(prompt = 'Continue the authorized preparation') {
      return await fire('before_agent_start', { reason: 'prompt', prompt }) as Prepared | undefined;
    },
    accept(prepared: Prepared) {
      entries.push({ id: `context-${++serial}`, type: 'custom_message', ...structuredClone(prepared.message) });
    },
    async handoff() { await commands.get('lead-guard')!('handoff', context); },
  };
}

test('lead activation requires an exact session pin and main identity, including depth-zero advisors', async t => {
  const f = await fixture(t);
  const original = f.entries;
  f.flags.delete('lead-guard-session');
  await f.fire('session_start');
  assert.equal(await f.prepare(), undefined);
  f.flags.set('lead-guard-session', 'another-session');
  await f.fire('session_switch', { reason: 'resume' });
  assert.equal(await f.prepare(), undefined);
  f.flags.set('lead-guard-session', 'lead-session');
  f.kind = 'sub';
  const before = [...f.statuses];
  await f.fire('session_start');
  assert.equal(await f.prepare(), undefined);
  assert.deepEqual(f.statuses, before);
  assert.deepEqual(f.notices, []);
  assert.deepEqual(f.entries, original);
  f.kind = 'main';
  await f.fire('session_start');
  assert.equal(f.notices.length, 1);
  assert.match(f.statuses.at(-1)!, /alignment unverified/);
  assert.deepEqual(f.forbidden, []);
});

test('resume and unchanged lifecycle boundaries dedupe using durable branch state', async t => {
  const f = await fixture(t);
  await f.fire('session_start');
  const saved = f.entries;
  f.bind(); // A fresh factory/process with the same persisted active branch.
  await f.fire('session_start');
  await f.fire('session_switch', { reason: 'resume' });
  await f.fire('session_before_compact');
  await f.fire('auto_compaction_start', { action: 'handoff' });
  await f.handoff();
  assert.equal(await f.fire('session_stop'), undefined);
  assert.equal(f.notices.length, 1);
  assert.deepEqual(f.entries, saved);
});

test('content changes are detected with preserved mtimes while timestamp-only changes do not nag', async t => {
  const f = await fixture(t);
  await f.fire('session_start');
  const board = join(f.root, 'WORKING-ON.md');
  const original = await stat(board);
  await utimes(board, new Date(0), new Date(0));
  await f.fire('session_stop');
  assert.equal(f.notices.length, 1);
  await writeFile(board, '# PREP-B: product PAUSED\n');
  await utimes(board, original.atime, original.mtime);
  await f.fire('session_stop');
  assert.equal(f.notices.length, 2);
  await f.fire('session_stop');
  assert.equal(f.notices.length, 2);
  assert.equal(await readFile(board, 'utf8'), '# PREP-B: product PAUSED\n');
});

test('missing board, checkpoint, decisions and todos warn without claiming reconciliation or creating files', async t => {
  const f = await fixture(t);
  await rm(join(f.root, 'WORKING-ON.md'));
  await rm(join(f.root, 'docs/plans/lead-preparation.md'));
  await rm(join(f.root, 'docs/plans/decisions.md'));
  f.entries = [];
  await f.fire('session_start');
  const prepared = await f.prepare();
  assert.ok(prepared);
  const warnings = prepared.message.details.warnings as string[];
  assert.equal(warnings.length, 4);
  assert.ok(warnings.some(value => value.includes('WORKING-ON.md') && value.includes('missing')));
  assert.ok(warnings.some(value => value.includes('lead-preparation.md') && value.includes('missing')));
  assert.ok(warnings.some(value => value.includes('decisions.md') && value.includes('missing')));
  assert.match(f.statuses.at(-1)!, /alignment unverified/);
  assert.equal(f.notices[0].level, 'warning');
  await assert.rejects(readFile(join(f.root, 'WORKING-ON.md')), { code: 'ENOENT' });
  assert.deepEqual(f.forbidden, []);
});

test('only canonical committed todo snapshots change the material revision', async t => {
  const f = await fixture(t);
  await f.fire('session_start');
  for (const [id, op, isError] of [['view', 'view', false], ['failed', 'done', true]] as const) {
    f.push({ id, type: 'message', message: { role: 'toolResult', toolName: 'todo', isError, details: { op, phases: phases('completed') } } });
  }
  await f.fire('session_stop');
  assert.equal(f.notices.length, 1);
  f.push({ id: 'committed', type: 'message', message: { role: 'toolResult', toolName: 'todo', isError: false,
    details: { op: 'block', phases: phases('blocked', 'Await independent evidence') } } });
  await f.fire('session_stop');
  assert.equal(f.notices.length, 2);
  f.push({ id: 'ui-copy', type: 'custom', customType: 'user_todo_edit', data: { phases: phases('blocked', 'Await independent evidence') } });
  await f.fire('session_stop');
  assert.equal(f.notices.length, 2);
  f.push({ id: 'changed-blocker', type: 'custom', customType: 'user_todo_edit', data: { phases: phases('blocked', 'User pause') } });
  await f.fire('session_stop');
  assert.equal(f.notices.length, 3);
});

test('cancelled policy preparation is not a receipt and accepted reminders dedupe on retry', async t => {
  const f = await fixture(t);
  await f.fire('session_start');
  const saved = f.entries;
  const first = await f.prepare();
  assert.ok(first);
  assert.equal(first.message.display, false);
  assert.deepEqual(f.entries, saved); // Simulate the host cancelling this preparation.
  const retry = await f.prepare();
  assert.deepEqual(retry, first);
  f.accept(retry!);
  assert.equal(await f.prepare(), undefined);
  assert.ok(await f.prepare('Different authorized request'));
  assert.equal(await f.fire('session_stop'), undefined);
  assert.equal(f.notices.length, 1);
  assert.deepEqual(f.forbidden, []);
});

test('tree rewind uses ancestor receipts, and a new session branch requires explicit designation', async t => {
  const f = await fixture(t);
  await f.fire('session_start');
  const ancestor = f.entries;
  f.push({ id: 'new-todo', type: 'custom', customType: 'user_todo_edit', data: { phases: phases('blocked', 'Later branch') } });
  await f.fire('session_stop');
  assert.equal(f.notices.length, 2);
  f.entries = ancestor;
  await f.fire('session_tree');
  assert.equal(f.notices.length, 2);
  f.session = 'fork-session';
  await f.fire('session_branch', { reason: 'fork' });
  assert.equal(await f.prepare(), undefined);
  assert.deepEqual(f.entries, ancestor);
  f.flags.set('lead-guard-session', 'fork-session');
  f.bind(); // Explicitly restart/designate the branch, never inherit authority from its journal.
  await f.fire('session_start');
  assert.equal(f.notices.length, 3);
  assert.ok(await f.prepare());
});

test('compaction revisions refresh context once and unchanged yield does not force work', async t => {
  const f = await fixture(t);
  await f.fire('session_start');
  const prior = await f.prepare();
  assert.ok(prior);
  f.accept(prior);
  f.push({ id: 'compact-1', type: 'compaction' });
  await f.fire('session_compact');
  await f.fire('session_compact');
  await f.fire('session_stop');
  assert.equal(f.notices.length, 2);
  const after = await f.prepare();
  assert.ok(after);
  assert.notEqual(after.message.details.revision, prior.message.details.revision);
  assert.equal(after.message.details.material, prior.message.details.material);
  f.accept(after);
  await f.fire('auto_compaction_start', { action: 'handoff' });
  assert.equal(f.notices.length, 2);
  await writeFile(join(f.root, 'docs/plans/decisions.md'), '# Decision 2: retain PREP-only authority\n');
  await f.handoff();
  await f.handoff();
  assert.equal(f.notices.length, 3);
});

test('PAUSED and PREP stay passive and aborted stop cannot resume or block the agent', async t => {
  const f = await fixture(t);
  const originalTodos = f.entries;
  const board = await readFile(join(f.root, 'WORKING-ON.md'), 'utf8');
  await f.fire('session_start');
  await writeFile(join(f.root, 'docs/plans/decisions.md'), '# Still paused\n');
  const stopped = new AbortController();
  stopped.abort();
  assert.equal(await f.fire('session_stop', { signal: stopped.signal }), undefined);
  assert.equal(f.notices.length, 1);
  assert.equal(await f.fire('session_before_compact', { signal: stopped.signal }), undefined);
  assert.equal(await f.fire('before_agent_start', { reason: 'revive', prompt: 'resume' }), undefined);
  assert.equal(await f.fire('before_agent_start', { reason: 'prompt', prompt: '  ' }), undefined);
  const paused = await f.prepare('Please stay paused');
  assert.ok(paused); // Passive context only, not a turn request or instruction to proceed.
  assert.equal(await f.fire('session_stop'), undefined);
  assert.equal(f.notices.length, 2);
  assert.deepEqual(f.entries.filter(entry => entry.customType === 'user_todo_edit'), originalTodos);
  assert.equal(await readFile(join(f.root, 'WORKING-ON.md'), 'utf8'), board);
  assert.deepEqual(f.forbidden, []);
});

test('session switching invalidates an in-flight observation instead of persisting into the new session', async t => {
  const f = await fixture(t);
  const original = f.entries;
  const pending = f.fire('session_start');
  f.session = 'non-lead';
  await f.fire('session_switch', { reason: 'new' });
  await pending;
  assert.deepEqual(f.entries, original);
  assert.deepEqual(f.notices, []);
  assert.match(f.statuses.at(-1)!, /inactive/);
});

test('persistence failure remains visible and never turns a reminder into a conversation gate', async t => {
  const f = await fixture(t);
  const original = f.entries;
  f.failPersistence = true;
  assert.equal(await f.fire('session_start'), undefined);
  assert.match(f.statuses.at(-1)!, /unavailable/);
  assert.equal(f.notices.at(-1)!.level, 'warning');
  assert.deepEqual(f.entries, original);
  assert.ok(await f.prepare());
  assert.deepEqual(f.forbidden, []);
});

test('undesignated or invalid decision paths are warnings rather than guessed current decisions', async t => {
  const f = await fixture(t);
  f.flags.delete('lead-guard-decisions');
  const missing = await f.prepare();
  assert.ok(missing);
  assert.ok((missing.message.details.warnings as string[]).some(warning => warning.includes('not designated')));
  f.flags.set('lead-guard-decisions', '../outside.md');
  const outside = await f.prepare();
  assert.ok(outside);
  assert.ok((outside.message.details.warnings as string[]).some(warning => warning.includes('repository-relative')));
  f.flags.set('lead-guard-decisions', 'docs/plans');
  const directory = await f.prepare();
  assert.ok(directory);
  assert.ok((directory.message.details.warnings as string[]).some(warning => warning.includes('regular file')));
  await writeFile(join(f.root, 'large.md'), Buffer.alloc(1024 * 1024 + 1));
  f.flags.set('lead-guard-decisions', 'large.md');
  const large = await f.prepare();
  assert.ok(large);
  assert.ok((large.message.details.warnings as string[]).some(warning => warning.includes('1 MiB')));
});
