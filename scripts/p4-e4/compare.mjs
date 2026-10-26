// Lead-run only. No provider endpoints, SDKs, credentials or source-repository writes.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { execFile } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { createLegacy } from './legacy.mjs';
import { D8_FIXTURE_PATH, runD8ParserCases } from './d8-parser.mjs';

const exec = promisify(execFile);
const ROOT = fileURLToPath(new URL('../../', import.meta.url));
const FIXTURE = path.join(ROOT, 'transgate/fixtures/ai/e4/corpus.json');
const MARKER = 'e4-synthetic-not-a-secret';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const clone = value => JSON.parse(JSON.stringify(value));
const argv = process.argv.slice(2);
const options = {};
for (let n = 0; n < argv.length; n += 2) {
  if (!['--source', '--example', '--prompts', '--out'].includes(argv[n]) || !argv[n + 1]) {
    throw new Error('Usage: node scripts/p4-e4/compare.mjs --source /path/to/frontend --example target/debug/examples/e4_capture --prompts target/debug/examples/e4_prompts --out target/p4-e4');
  }
  if (options[argv[n]]) throw new Error(`Duplicate option ${argv[n]}`);
  options[argv[n]] = argv[n + 1];
}
if (!options['--source'] || !options['--example'] || !options['--prompts'] || !options['--out']) {
  throw new Error('All of --source, --example, --prompts and --out are required; no provider/default executable.');
}
const sourceRoot = await realpath(options['--source']);
const executable = await realpath(options['--example']);
const promptExecutable = await realpath(options['--prompts']);
const out = path.resolve(ROOT, options['--out']);
// Artifacts go only under this checkout's target tree, never into the old app.
assert.equal(path.dirname(out), path.join(ROOT, 'target'), 'Output must be a direct child of checkout target/');
assert.ok(!out.startsWith(sourceRoot + path.sep), 'Source repository is read-only');
const resolvedTarget = await realpath(path.join(ROOT, 'target'));
assert.ok(!resolvedTarget.startsWith(sourceRoot + path.sep), 'Target symlink enters source repository');
await mkdir(out, { recursive: true });
const resolvedOut = await realpath(out);
assert.ok(resolvedOut.startsWith(resolvedTarget + path.sep), 'Output symlink escapes target/');
assert.ok(!resolvedOut.startsWith(sourceRoot + path.sep), 'Output symlink enters source repository');
// A fresh artifact directory avoids mixing old and new capture evidence.
const runDir = await import('node:fs/promises').then(fs => fs.mkdtemp(path.join(resolvedOut, 'run-')));
const corpusBytes = await readFile(FIXTURE);
const corpus = JSON.parse(corpusBytes);
const results = [];
let failure = null;
let active = null;
const sockets = new Set();
const server = http.createServer(async (req, res) => {
  try {
    assert.ok(active, 'Unexpected loopback request');
    const current = active;
    assert.equal(current.requests.length, 0, 'No retries or duplicate dispatch allowed');
    const pieces = [];
    let length = 0;
    for await (const piece of req) {
      length += piece.length;
      assert.ok(length <= 16 * 1024 * 1024, 'Unexpected oversize local request');
      pieces.push(piece);
    }
    const body = Buffer.concat(pieces).toString('utf8');
    current.requests.push({ method: req.method, path: req.url, headers: req.headers, body, json: JSON.parse(body) });
    res.writeHead(current.status, { 'Content-Type': current.status === 200 ? 'text/event-stream' : 'application/json', 'Connection': 'close' });
    let offset = 0;
    let index = 0;
    while (offset < current.bytes.length && !res.destroyed) {
      const size = current.schedule[index++ % current.schedule.length];
      const next = current.bytes.subarray(offset, offset + size);
      offset += next.length;
      res.write(next);
      // Real writes; the kernel may coalesce reads. No claim of forced network read boundaries.
      if (current.schedule.length > 1) await new Promise(resolve => setImmediate(resolve));
    }
    res.end();
  } catch (error) {
    if (active) active.serverError = error;
    res.destroy();
  }
});
server.on('connection', socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });

// The original source fetch is retained, but it can reach only this exact local listener.
const legacy = createLegacy(sourceRoot, async (url, init) => {
  const parsed = new URL(url);
  assert.equal(parsed.origin, origin, 'Legacy fetch attempted a non-fixture origin');
  assert.ok(['/chat/completions', '/v1/chat/completions'].includes(parsed.pathname));
  assert.equal(init.headers.Authorization, `Bearer ${MARKER}`);
  return fetch(url, { ...init, redirect: 'error', signal: AbortSignal.timeout(10000) });
});
await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
const origin = `http://127.0.0.1:${server.address().port}`;

function completionText(testCase) {
  return testCase.kind === 'analysis'
    ? testCase.documents.map(document => JSON.stringify(document)).join('\n---\n')
    : testCase.answer;
}
function sseBytes(text, { finish = 'stop', done = true, multiline = false, malformed = false } = {}) {
  const eol = multiline ? '\r\n' : '\n';
  const records = [];
  if (multiline) {
    records.push(': synthetic CRLF comment', 'event: message', 'id: e4-synthetic');
    records.push('data: {"choices":', `data: [{"delta":{"content":${JSON.stringify(text)}}}]}`, '');
  } else {
    // Deliberately put Unicode inside an event and write its bytes in tiny pieces later.
    for (let offset = 0; offset < text.length; offset += 23) {
      records.push(`data: ${JSON.stringify({ id: 'e4-synthetic', choices: [{ delta: { content: text.slice(offset, offset + 23) } }] })}`, '');
    }
  }
  if (malformed) records.push('data: {this is not json}', '');
  if (finish !== null) records.push(`data: ${JSON.stringify({ choices: [{ delta: {}, finish_reason: finish }] })}`, '');
  if (done) records.push('data: [DONE]', '');
  return Buffer.from(records.join(eol) + eol, 'utf8');
}
async function save(name, value) {
  await writeFile(path.join(runDir, name), JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
}
function sourceExpected(testCase) {
  return {
    model: testCase.provider === 'openai' ? 'gpt-3.5-turbo' : 'deepseek-chat',
    temperature: 0.3, max_tokens: 1000, stream: true,
    ...(testCase.provider === 'openai' ? {} : { store: false, ephemeral: true }),
    ...(testCase.kind === 'analysis' ? { response_format: { type: 'json_object' } } : {}),
  };
}
function checkRequest(request, testCase, expectedBody = null) {
  assert.equal(request.method, 'POST');
  assert.equal(request.path, testCase.provider === 'openai' ? '/v1/chat/completions' : '/chat/completions');
  assert.equal(request.headers.authorization, `Bearer ${MARKER}`);
  assert.equal(request.headers['content-type'], 'application/json');
  if (expectedBody) assert.deepEqual(request.json, expectedBody, 'Transport changed source messages/model/options');
  else {
    const { messages, ...flags } = request.json;
    assert.deepEqual(flags, sourceExpected(testCase), 'Source request changed from inventoried baseline');
    assert.ok(Array.isArray(messages) && messages.length >= 2);
    assert.equal(messages[0].role, 'system');
  }
}
async function dispatch(name, testCase, fixture, invoke) {
  const capture = { ...fixture, requests: [], serverError: null };
  active = capture;
  let value;
  try {
    value = await invoke();
  } catch (error) {
    await save(`${name}.json`, { invocation_error: error.message, requests: capture.requests, response_sha256: hash(fixture.bytes) });
    throw error;
  } finally {
    active = null;
  }
  await save(`${name}.json`, { value, requests: capture.requests, response_sha256: hash(fixture.bytes) });
  if (capture.serverError) throw capture.serverError;
  assert.equal(capture.requests.length, 1, 'Expected exactly one actual HTTP dispatch');
  return { value, request: capture.requests[0] };
}
async function candidate(name, testCase, fixture, body) {
  const input = {
    provider: testCase.provider, base_url: origin + (testCase.provider === 'openai' ? '/v1' : ''),
    api_key: MARKER, model: body.model, messages: body.messages,
    response_format: body.response_format?.type ?? null,
  };
  const inputPath = path.join(runDir, `${name}.input.json`);
  const outputPath = path.join(runDir, `${name}.output.json`);
  await writeFile(inputPath, JSON.stringify(input, null, 2) + '\n', { flag: 'wx' });
  return dispatch(name, testCase, fixture, async () => {
    const { stdout, stderr } = await exec(executable, [inputPath, outputPath], {
      cwd: ROOT, timeout: 15000, maxBuffer: 1024 * 1024,
      // No inherited provider credentials, model overrides or proxies.
      env: { PATH: process.env.PATH ?? '/usr/bin:/bin', RUST_BACKTRACE: '1', NO_PROXY: '*' },
    });
    const value = JSON.parse(await readFile(outputPath, 'utf8'));
    assert.equal(typeof value.content, 'string');
    assert.ok(Object.hasOwn(value, 'error'));
    assert.equal(typeof value.terminal, 'boolean');
    return { ...value, stdout, stderr };
  });
}
function canonicalCase(testCase) {
  return { ...testCase, lines: testCase.lines.map((line, index) => ({
    id: index + 1, job: 1, account: '00000000-0000-0000-0000-000000000004',
    start_time: '00:00:00', end_time: '00:00:01', created_at: 0,
    line: line.text || line.content || line.line || '',
    speaker: line.speaker || line.speaker_id || null,
    corrected_text: null, split_on_speaker_change: null, language: null, translation_status: null,
  })) };
}
async function capturePrompts(name, testCase) {
  // Only canonical fixture Lines cross both implementations, not source-built prompts.
  const input = {
    kind: testCase.kind, lines: testCase.lines, language: testCase.language ?? null,
    detailed: Boolean(testCase.detailed), question: testCase.question ?? null, history: testCase.history ?? [],
  };
  const inputPath = path.join(runDir, `${name}.prompt.input.json`);
  const outputPath = path.join(runDir, `${name}.prompt.output.json`);
  await writeFile(inputPath, JSON.stringify(input, null, 2) + '\n', { flag: 'wx' });
  const { stdout, stderr } = await exec(promptExecutable, [inputPath, outputPath], {
    cwd: ROOT, timeout: 15000, maxBuffer: 1024 * 1024,
    env: { PATH: process.env.PATH ?? '/usr/bin:/bin', RUST_BACKTRACE: '1' },
  });
  const candidate = JSON.parse(await readFile(outputPath, 'utf8'));
  return { messages: candidate.messages, analysis_input_estimated_tokens: candidate.analysis_input_estimated_tokens,
    stdout, stderr, canonical_input_sha256: hash(JSON.stringify(input)) };
}
async function comparePrompts(name, testCase, sourceMessages) {
  const candidate = await capturePrompts(name, testCase);
  await save(`${name}.prompt.comparison.json`, {
    group: 'independent-Rust-product-builders-versus-source-manager',
    source_messages: sourceMessages, candidate_messages: candidate.messages,
    stdout: candidate.stdout, stderr: candidate.stderr, canonical_input_sha256: candidate.canonical_input_sha256,
  });
  assert.deepEqual(candidate.messages, sourceMessages, 'Actual Rust product prompt builders differ from source-produced prompt bytes');
  return candidate.messages;
}
async function d8LiteralSubstitution(schedule, suffix) {
  const testCase = canonicalCase({ ...corpus.d8_literal_substitution, kind: 'analysis', provider: 'deepseek' });
  const text = completionText(testCase);
  const fixture = { bytes: sseBytes(text), status: 200, schedule };
  const name = `${testCase.id}-${suffix}`;
  await writeFile(path.join(runDir, `${name}.response.sse`), fixture.bytes, { flag: 'wx' });
  const old = await dispatch(`${name}-fetch`, testCase, fixture, () => legacy.run(testCase, origin));
  checkRequest(old.request, testCase);
  assert.equal(old.value.error, null);
  assert.equal(old.value.content, text);

  // Baseline transport MUST receive the actual old request, including expanded directives.
  const baselineRust = await candidate(`${name}-old-messages-http`, testCase, fixture, old.request.json);
  checkRequest(baselineRust.request, testCase, old.request.json);
  assert.equal(baselineRust.value.error, null);
  assert.equal(baselineRust.value.terminal, true);
  assert.equal(baselineRust.value.finish_reason, 'stop');
  assert.equal(baselineRust.value.content, text);
  results.push({ name: `${name}-old-messages`, group: 'exact-old-fetch-versus-plain-http',
    request_equal: true, content_equal: true, response_sha256: hash(fixture.bytes) });

  const template = legacy.analysisTemplate();
  const marker = '{{transcript}}';
  const markerIndex = template.user.indexOf(marker);
  assert.ok(markerIndex >= 0);
  assert.equal(markerIndex, template.user.lastIndexOf(marker), 'D8 requires the actual single source placeholder');
  const prefix = template.user.slice(0, markerIndex);
  const tail = template.user.slice(markerIndex + marker.length);
  const transcript = testCase.lines.map(line => `${line.speaker || 'Speaker'}: ${line.line}`).join('\n');
  // Host JS specifies the legacy replacement-string semantics; concatenation specifies
  // the independently approved literal result. Neither oracle copies Rust prompt wording.
  const expectedOld = template.user.replace(marker, transcript);
  const expectedLiteral = prefix + transcript + tail;
  const directives = [
    { directive: '$$', literal: '[double=$$]', expanded: '[double=$]' },
    { directive: '$&', literal: '[match=$&]', expanded: `[match=${marker}]` },
    { directive: '$`', literal: '[prefix=$`]', expanded: `[prefix=${prefix}]` },
    { directive: "$'", literal: "[suffix=$']", expanded: `[suffix=${tail}]` },
  ];
  for (const { literal, expanded } of directives) {
    assert.ok(transcript.includes(literal), `D8 fixture missing ${literal}`);
    assert.ok(expectedOld.includes(expanded), 'D8 legacy directive expansion missing');
    assert.ok(expectedLiteral.includes(literal), 'D8 corrected directive was not literal');
  }
  assert.notEqual(expectedOld, expectedLiteral, 'D8 must exercise a real source/Rust difference');
  const expectedOldMessages = [{ role: 'system', content: template.system }, { role: 'user', content: expectedOld }];
  const expectedLiteralMessages = [{ role: 'system', content: template.system }, { role: 'user', content: expectedLiteral }];
  const rustPrompts = await capturePrompts(`${name}-D8-literal`, testCase);
  await save(`${name}-D8-literal.prompt.comparison.json`, {
    group: 'D8-literal-transcript-substitution-NOT-prompt-parity',
    decision: 'docs/plans/p4-ai/decisions.md D8', directives,
    source_messages: old.request.json.messages, candidate_messages: rustPrompts.messages,
    expected_old_messages: expectedOldMessages, expected_literal_messages: expectedLiteralMessages,
    canonical_input_sha256: rustPrompts.canonical_input_sha256,
    source_template_sha256: hash(template.user), stdout: rustPrompts.stdout, stderr: rustPrompts.stderr,
  });
  assert.deepEqual(old.request.json.messages, expectedOldMessages, 'D8 old source must retain actual JS directive expansion');
  assert.deepEqual(rustPrompts.messages, expectedLiteralMessages, 'D8 Rust must contain exact literal transcript bytes');
  assert.notDeepEqual(rustPrompts.messages, old.request.json.messages, 'D8 cannot be reported as prompt equality');
  const sourceMetadata = await legacy.sourceAnalysis([text], testCase.lines);
  const correctedTokens = Math.round(transcript.length / 4); // JS UTF-16 units; analysis is unmetered.
  await save(`${name}-D8-unmetered-token-metadata.json`, {
    group: 'D8-actual-transcript-token-metadata-NOT-chat-quota', source: sourceMetadata,
    actual_transcript: transcript, expected_corrected_tokens: correctedTokens,
    rust_corrected_tokens: rustPrompts.analysis_input_estimated_tokens,
  });
  assert.equal(sourceMetadata.rawContent, 'Speaker: ', 'Old estimate must still use the nonexistent canonical text columns');
  assert.equal(sourceMetadata.tokensUsed, Math.round(sourceMetadata.rawContent.length / 4));
  assert.equal(rustPrompts.analysis_input_estimated_tokens, correctedTokens);
  assert.notEqual(rustPrompts.analysis_input_estimated_tokens, sourceMetadata.tokensUsed);

  // Isolate D8: keep json_object and every other baseline option unchanged.
  // The separately named D1 experiments are the only ones omitting response_format.
  const correctedBody = { ...old.request.json, messages: rustPrompts.messages };
  const correctedRust = await candidate(`${name}-D8-literal-http`, testCase, fixture, correctedBody);
  checkRequest(correctedRust.request, testCase, correctedBody);
  assert.equal(correctedRust.value.error, null);
  assert.equal(correctedRust.value.terminal, true);
  assert.equal(correctedRust.value.finish_reason, 'stop');
  assert.equal(correctedRust.value.content, text);
  results.push({ name: `${name}-D8-literal`, group: 'D8-literal-transcript-substitution-NOT-prompt-parity',
    only_request_change: 'messages[1].content: literal transcript rather than JS replacement directives',
    prompt_equal: false, named_difference_asserted: true, response_sha256: hash(fixture.bytes),
    source_user_sha256: hash(expectedOld), corrected_user_sha256: hash(expectedLiteral) });
}
async function baseline(testCase, schedule, suffix) {
  testCase = canonicalCase(testCase);
  const text = completionText(testCase);
  const fixture = { bytes: sseBytes(text), status: 200, schedule };
  const name = `${testCase.id}-${testCase.provider}-${suffix}`;
  await writeFile(path.join(runDir, `${name}.response.sse`), fixture.bytes, { flag: 'wx' });
  const old = await dispatch(`${name}-fetch`, testCase, fixture,
    () => legacy.run(testCase, origin + (testCase.provider === 'openai' ? '/v1' : '')));
  checkRequest(old.request, testCase);
  assert.equal(old.value.error, null, 'Source baseline must not fail');
  assert.equal(old.value.content, text, 'Source fetch changed fixture content');
  await comparePrompts(name, testCase, old.request.json.messages);
  const rust = await candidate(`${name}-http`, testCase, fixture, old.request.json);
  checkRequest(rust.request, testCase, old.request.json);
  assert.equal(rust.value.error, null, 'Candidate baseline must not fail');
  assert.equal(rust.value.finish_reason, 'stop');
  assert.equal(rust.value.content, text, 'Candidate changed response content');
  assert.equal(rust.value.terminal, true);
  results.push({ name, group: 'exact-old-fetch-versus-plain-http', prompt_comparison: 'independent-Rust-product-builders-versus-source-manager', prompt_equal: true, response_sha256: hash(fixture.bytes), request_equal: true, content_equal: true });

  if (testCase.kind === 'analysis') {
    const corrected = clone(old.request.json);
    delete corrected.response_format;
    const correctedOld = await dispatch(`${name}-D1-omitted-mode-fetch`, testCase, fixture,
      () => legacy.run(testCase, origin + (testCase.provider === 'openai' ? '/v1' : ''), true));
    checkRequest(correctedOld.request, testCase, corrected);
    const correctedRust = await candidate(`${name}-D1-omitted-mode-http`, testCase, fixture, corrected);
    checkRequest(correctedRust.request, testCase, corrected);
    assert.equal(correctedOld.value.error, null);
    assert.equal(correctedRust.value.error, null);
    assert.equal(correctedRust.value.finish_reason, 'stop');
    assert.equal(correctedRust.value.terminal, true);
    assert.equal(correctedOld.value.content, text);
    assert.equal(correctedRust.value.content, text);
    results.push({ name: `${name}-D1-omitted-mode`, group: 'D1-framing-correction-NOT-baseline-parity', only_request_change: 'response_format omitted', response_sha256: hash(fixture.bytes), content_equal: true });
  }
}
async function protocolCase(id, options, expectation) {
  const testCase = canonicalCase({ ...corpus.prose[0], id, provider: 'deepseek' });
  const text = completionText(testCase);
  const fixture = { bytes: sseBytes(text, options), status: 200, schedule: [1, 2, 7, 31] };
  if (options.status) { fixture.status = options.status; fixture.bytes = Buffer.from('{"error":{"message":"synthetic local refusal"}}'); }
  const name = `protocol-${id}`;
  await writeFile(path.join(runDir, `${name}.response.sse`), fixture.bytes, { flag: 'wx' });
  const old = await dispatch(`${name}-fetch`, testCase, fixture, () => legacy.run(testCase, origin));
  checkRequest(old.request, testCase);
  const rust = await candidate(`${name}-http`, testCase, fixture, old.request.json);
  checkRequest(rust.request, testCase, old.request.json);
  assert.equal(Boolean(old.value.error), expectation.oldError);
  assert.equal(old.value.content, expectation.oldEmpty || expectation.oldError ? '' : text);
  assert.equal(Boolean(rust.value.error), Boolean(expectation.rustError));
  if (expectation.rustContent) assert.equal(rust.value.content, text);
  if (expectation.finish !== undefined) assert.equal(rust.value.finish_reason, expectation.finish);
  assert.equal(rust.value.terminal, !expectation.rustError);
  results.push({ name, group: expectation.group, response_sha256: hash(fixture.bytes), old: old.value, candidate: rust.value,
    candidate_successful_stop: !rust.value.error && rust.value.terminal && rust.value.finish_reason === 'stop' });
}

try {
  await save('provenance.json', {
    captured_at: new Date().toISOString(),
    fixture: { path: path.relative(ROOT, FIXTURE), sha256: hash(corpusBytes), ...corpus.provenance },
    d8_parser_fixture: { path: path.relative(ROOT, D8_FIXTURE_PATH), sha256: hash(await readFile(D8_FIXTURE_PATH)) },
    executable: { path: executable, sha256: hash(await readFile(executable)) },
    prompt_executable: { path: promptExecutable, sha256: hash(await readFile(promptExecutable)) },
    rust_prompt_sources: await Promise.all([
      'transgate/core/src/ai/prompts.rs', 'transgate/core/src/ai/sanitize.rs',
      'transgate/core/src/ai/analysis.rs', 'transgate/core/src/ai/analysis/parser.rs',
      'transgate/core/examples/e4_prompts.rs',
    ].map(async file => ({ path: file, sha256: hash(await readFile(path.join(ROOT, file))) }))),
    source: legacy.provenance(), node: process.version,
    claims: 'Offline source request and byte replay only; not live-provider acceptance or parser/product/storage evidence.',
  });
  const cases = [
    ...corpus.analysis.map(testCase => ({ ...testCase, kind: 'analysis', language: 'tr' })),
    ...corpus.prose,
  ];
  for (const testCase of cases) {
    for (const [suffix, schedule] of [['coalesced', [1048576]], ['fragmented', [1, 7, 3, 97]]]) {
      await baseline({ ...testCase, provider: 'deepseek' }, schedule, suffix);
    }
  }
  // Explicit source alternate, never fallback or a replacement for DeepSeek.
  await baseline({ ...corpus.prose[0], provider: 'openai' }, [1, 7, 3, 97], 'explicit-alternate');
  await baseline({ ...corpus.analysis[0], kind: 'analysis', provider: 'openai' }, [1048576], 'explicit-alternate');
  for (const [suffix, schedule] of [['coalesced', [1048576]], ['fragmented', [1, 7, 3, 97]]]) {
    await d8LiteralSubstitution(schedule, suffix);
  }
  results.push(...await runD8ParserCases({
    legacy, promptExecutable, runDir, lines: canonicalCase(corpus.d8_literal_substitution).lines,
  }));
  await protocolCase('CRLF-multiline-data', { multiline: true }, { oldError: false, oldEmpty: true, rustError: false, rustContent: true, finish: 'stop', group: 'named-SSE-decoding-correction-NOT-content-parity' });
  await protocolCase('unexplained-EOF', { finish: null, done: false }, { oldError: false, rustError: true, rustContent: true, group: 'named-terminal-correction-NOT-success-parity' });
  await protocolCase('DONE-without-reason', { finish: null }, { oldError: false, rustError: true, rustContent: true, group: 'named-terminal-correction-NOT-success-parity' });
  await protocolCase('reason-without-DONE', { done: false }, { oldError: false, rustError: true, rustContent: true, group: 'named-terminal-correction-NOT-success-parity' });
  await protocolCase('length-not-success', { finish: 'length' }, { oldError: false, rustContent: true, finish: 'length', group: 'named-terminal-correction-NOT-success-parity' });
  await protocolCase('filter-not-success', { finish: 'content_filter' }, { oldError: false, rustContent: true, finish: 'content_filter', group: 'named-terminal-correction-NOT-success-parity' });
  await protocolCase('tool-calls-not-success', { finish: 'tool_calls' }, { oldError: false, rustContent: true, finish: 'tool_calls', group: 'named-terminal-correction-NOT-success-parity' });
  await protocolCase('malformed-record', { malformed: true }, { oldError: false, rustError: true, rustContent: true, group: 'named-protocol-correction-NOT-success-parity' });
  for (const status of [429, 503]) {
    await protocolCase(`HTTP-${status}`, { status }, { oldError: true, rustError: true, group: 'both-refuse-not-error-taxonomy-parity' });
  }
} catch (error) {
  failure = { message: error.message, stack: error.stack };
  process.exitCode = 1;
} finally {
  for (const socket of sockets) socket.destroy();
  await new Promise(resolve => server.close(resolve));
  await save('report.json', {
    status: failure ? 'FAILED' : 'PASSED-OFFLINE-ONLY', failure, results,
    source: legacy.provenance(), fixture_sha256: hash(corpusBytes),
    not_proved: ['Live model availability/flags', 'Provider quality/multilingual correctness', 'Provider retention/region/cost/cancel', 'Transgate route auth/quota/consent', 'Full six-shape parser regression acceptance', 'Persisted/final product behavior', 'Rendered or binding parity', 'Deployed/imported prompt/config provenance'],
  });
}
console.log(JSON.stringify({ status: failure ? 'FAILED' : 'PASSED-OFFLINE-ONLY', cases: results.length, artifacts: runDir, failure }, null, 2));
