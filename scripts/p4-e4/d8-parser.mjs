// Lead-run source/Rust differentials. No HTTP, credentials, copied parser or expected wording.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const exec = promisify(execFile);
export const D8_FIXTURE_PATH = fileURLToPath(new URL('../../transgate/fixtures/ai/e4/d8-parser.json', import.meta.url));
export async function runD8ParserCases({ legacy, promptExecutable, runDir, lines }) {
  const bytes = await readFile(D8_FIXTURE_PATH);
  const corpus = JSON.parse(bytes);
  const rows = [];
  const save = (name, value) => writeFile(path.join(runDir, name), JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
  await save('D8-parser-provenance.json', { ...corpus.provenance,
    sha256: createHash('sha256').update(bytes).digest('hex') });
  for (const testCase of corpus.cases) {
    const separator = testCase.separator ?? '\n---\n';
    const documents = testCase.documents.map(document => JSON.stringify(document));
    const text = documents.join(separator);
    for (const mode of ['documents', 'fragmented']) {
      const chunks = mode === 'documents'
        ? documents.map((document, index) => document + (index + 1 < documents.length ? separator : ''))
        : Array.from(text);
      const name = `D8-parser-${testCase.id}-${mode}`;
      const source = await legacy.sourceAnalysis(chunks, lines);
      await save(`${name}.source.json`, source);
      assert.equal(source.ended, true);
      assert.equal(source.events.some(event => event.event === 'complete'), true, 'Source route must actually emit complete');
      assert.equal(source.events.some(event => event.event === 'error'), false);
      const detected = source.events.find(event => event.event === 'typeDetected');
      assert.equal(detected?.data.confidence, testCase.confidence);
      const sourceSummary = source.events.find(event => event.event === 'sectionReady' && event.data.sectionType === 'summary');
      if (testCase.source_summary_missing) assert.equal(sourceSummary, undefined, 'Old literal split must lose the quoted-marker summary');
      else assert.equal(sourceSummary?.data.data, testCase.summary);
      if (testCase.source_omitted_section) {
        assert.equal(source.events.some(event => event.event === 'sectionReady' && event.data.sectionType === testCase.source_omitted_section), false,
          'Falsy fallbacks ending undefined are omitted by the actual route');
      }
      let renderFiltered = null;
      if (testCase.source_emitted_invalid_section) {
        const event = source.events.find(event => event.event === 'sectionReady' && event.data.sectionType === testCase.source_emitted_invalid_section);
        assert.equal(event?.data.data, testCase.source_emitted_value, 'A falsy terminal operand must still be emitted');
        renderFiltered = legacy.renderActionItems(event.data.data);
        assert.equal(renderFiltered.length, 0, 'Actual source render helper must filter the nonarray payload');
      }
      if (testCase.pretype_task) {
        const sectionIndex = source.events.findIndex(event => event.event === 'sectionReady' && event.data.sectionType === 'actionItems');
        const detectedIndex = source.events.findIndex(event => event.event === 'typeDetected');
        assert.ok(sectionIndex >= 0 && sectionIndex < detectedIndex, 'Old source emits the section before detection');
      }
      const input = { kind: 'parse_analysis', lines, chunks, successful_stop: true,
        capture_progress: Boolean(testCase.pretype_task) && mode === 'documents' };
      const inputPath = path.join(runDir, `${name}.input.json`);
      const outputPath = path.join(runDir, `${name}.output.json`);
      await save(`${name}.input.json`, input);
      const { stdout, stderr } = await exec(promptExecutable, [inputPath, outputPath], {
        timeout: 15000, maxBuffer: 1024 * 1024,
        env: { PATH: process.env.PATH ?? '/usr/bin:/bin', RUST_BACKTRACE: '1' },
      });
      const report = JSON.parse(await readFile(outputPath, 'utf8'));
      const actual = report.analysis_parse;
      await save(`${name}.comparison.json`, { group: testCase.group, source, renderFiltered, candidate: actual, stdout, stderr });
      if (testCase.error) {
        assert.equal(actual.result, null);
        assert.equal(actual.failure.error, testCase.error, 'D8 rejects rather than silently hiding an invalid section');
        assert.equal(actual.failure.partial.summary, testCase.summary, 'Valid predecessor must survive interruption');
        assert.equal(actual.failure.partial.detected.confidence, testCase.confidence);
      } else {
        assert.equal(actual.push_error, null);
        assert.equal(actual.failure, null);
        assert.equal(actual.result.detected.confidence, testCase.confidence);
        assert.equal(actual.result.summary, testCase.summary);
        if (testCase.pretype_task) {
          assert.equal(actual.result.analysis.type, 'meeting');
          assert.equal(actual.result.analysis.actionItems[0].task, testCase.pretype_task);
          if (mode === 'documents') {
            assert.equal(actual.checkpoints[0].detected, null);
            assert.equal(actual.checkpoints[0].analysis, null, 'Pretype section is deferred, not validated against a guessed type');
          }
        }
      }
      rows.push({ name, group: testCase.group, source_completed: true,
        rust_outcome: testCase.error ? 'interrupted-with-valid-partial' : 'complete',
        source_render_filter_exercised: Boolean(testCase.source_emitted_invalid_section) });
    }
  }
  return rows;
}
