import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, symlink, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { captureSource, extractAuth, extractKeys, flattenStrings, repairGermanApp } from './capture.mjs';
import { LOCALES } from './validate.mjs';

// Existing TypeScript parser only; tests never execute the source application.
const require = createRequire(process.env.P5_SOURCE_FRONTEND
  ? path.join(process.env.P5_SOURCE_FRONTEND, 'package.json')
  : new URL('../../apps/web/package.json', import.meta.url));
const ts = require('typescript');
const authSource = () => `const locales = ${JSON.stringify(Object.fromEntries(LOCALES.map(locale => [locale, { sign_in: { email_label: `${locale} email`, social_provider_text: `${locale} {{provider}}` } }])))};`;

test('flattening preserves literal dotted keys but rejects ambiguous nested collisions', () => {
  const flat = flattenStrings({ 'label:email.address': 'a', nested: { button: 'b' } });
  assert.equal(flat['label:email.address'], 'a');
  assert.equal(flat['nested.button'], 'b');
  assert.throws(() => flattenStrings({ 'nested.button': 'a', nested: { button: 'b' } }), /Ambiguous/u);
  assert.throws(() => flattenStrings({ label: ['not', 'a', 'string'] }), /Non-string/u);
});

test('German repair is restricted to the six documented quotation boundaries', () => {
  const lines = Array.from({ length: 132 }, () => '');
  for (const line of [115, 118, 121, 124, 127, 130]) lines[line - 1] = `  ${JSON.stringify(`"example ${line}"`)}: "„Beispiel."",`;
  const input = lines.join('\n');
  const repaired = repairGermanApp(input);
  assert.deepEqual(repaired.repairs.map(record => record.line), [115, 118, 121, 124, 127, 130]);
  assert.equal(repaired.text.split('\n')[114], `  ${JSON.stringify('"example 115"')}: "„Beispiel.“",`);
  lines[117] = '  "different": "well-formed",';
  assert.throws(() => repairGermanApp(lines.join('\n')), /line 118/u);
  assert.equal(input.split('\n')[114].endsWith('."",'), true);
});

test('auth capture accepts only literal all-locale data and preserves provider interpolation', () => {
  const auth = extractAuth(ts, authSource());
  assert.equal(auth.el['sign_in.social_provider_text'], 'el {{provider}}');
  assert.equal(auth.az['sign_in.email_label'], 'az email');
  assert.throws(() => extractAuth(ts, 'const locales = {en: externalCall()};'), /non-literal/u);
  assert.throws(() => extractAuth(ts, 'const locales = {en:{sign_in:{email_label:"Email"}}};'), /all 16/u);
});

test('translation aliases use exact namespaces and collect plural variants without parsing source code as an app', () => {
  const english = { app: { 'total_one': '{{count}} item', 'total_other': '{{count}} items', 'label:literal.dot': 'Label' } };
  const result = extractKeys(ts, `
    const {t: translate} = useTranslation('app');
    const view = <><Head>{translate('seo-only')}</Head><p>{translate('total', {count:2})}</p></>;
    translate('label:literal.dot');
    translate(dynamicState);
  `, 'src/synthetic.tsx', english);
  assert.deepEqual(new Set(result.keys.map(entry => entry.key)), new Set(['total_one', 'total_other', 'label:literal.dot']));
  assert.ok(result.keys.every(entry => entry.ns === 'app'));
  assert.equal(result.dynamic.length, 1);
  assert.equal(result.dynamic[0].expression, 'dynamicState');
});

async function sourceFixture(run) {
  const parent = await mkdtemp(path.join(tmpdir(), 'p5-locales-source-'));
  const root = path.join(parent, 'source');
  try {
    await mkdir(path.join(root, 'src/pages'), { recursive: true });
    await writeFile(path.join(root, 'src/pages/signin.tsx'), authSource());
    await writeFile(path.join(root, 'src/view.tsx'), "const {t}=useTranslation('common'); t('welcome');");
    for (const locale of LOCALES) {
      await mkdir(path.join(root, `public/locales/${locale}`), { recursive: true });
      await writeFile(path.join(root, `public/locales/${locale}/common.json`), '{"welcome":"old","welcome":"new"}');
    }
    await run({ root, parent });
  } finally { await rm(parent, { recursive: true, force: true }); }
}

test('source duplicate members retain last-wins meaning with explicit provenance', async () => {
  await sourceFixture(async ({root}) => {
    const report = await captureSource({ sourceRoot: root, typescript: ts,
      scope: { namespaces: ['common'], entrypoints: ['src/view.tsx'], extraKeys: {} } });
    assert.equal(report.catalogs.en.common.welcome, 'new');
    assert.ok(report.provenance.duplicates.some(record => record.path === 'public/locales/en/common.json' && record.key === 'welcome'));
    assert.match(report.provenance.files.find(record => record.path === 'src/view.tsx').sha256, /^[a-f0-9]{64}$/u);
  });
});

test('empty source translations remain visible in baseline evidence without enabling fallback', async () => {
  await sourceFixture(async ({root}) => {
    await writeFile(path.join(root, 'public/locales/pt/common.json'), JSON.stringify({ welcome: '' }));
    await writeFile(path.join(root, 'public/locales/az/common.json'), JSON.stringify({ welcome: ' \t' }));
    const report = await captureSource({ sourceRoot: root, typescript: ts,
      scope: { namespaces: ['common'], entrypoints: ['src/view.tsx'] } });
    assert.deepEqual(report.fallback.baseline.filter(record => record.reason === 'source_empty_value'), [
      { locale: 'pt', ns: 'common', key: 'welcome', reason: 'source_empty_value', source: 'public/locales/pt/common.json' },
      { locale: 'az', ns: 'common', key: 'welcome', reason: 'source_empty_value', source: 'public/locales/az/common.json' },
    ]);
    assert.equal(report.catalogs.pt.common.welcome, '');
    assert.equal(report.catalogs.az.common.welcome, ' \t');
    assert.deepEqual(report.fallback.active, []);
  });
});

test('capture refuses traversal and symlink escape instead of reading unrelated source files', async () => {
  await sourceFixture(async ({root, parent}) => {
    const scope = { namespaces: ['common'], entrypoints: ['src/../../outside.tsx'] };
    await assert.rejects(captureSource({ sourceRoot: root, scope, typescript: ts }), /outside capture scope/u);
    await writeFile(path.join(parent, 'outside.tsx'), 'secret outside the capture root');
    await symlink(path.join(parent, 'outside.tsx'), path.join(root, 'src/escape.tsx'));
    await assert.rejects(captureSource({ sourceRoot: root, typescript: ts,
      scope: { namespaces: ['common'], entrypoints: ['src/escape.tsx'] } }), /symlink leaves root/u);
  });
});

test('a changed pinned source snapshot fails instead of inventing new import provenance', async () => {
  await sourceFixture(async ({root}) => {
    await assert.rejects(captureSource({ sourceRoot: root, typescript: ts,
      scope: { namespaces: ['common'], entrypoints: ['src/view.tsx'], sourceHashes: { 'src/view.tsx': '0'.repeat(64) } } }), /Source snapshot changed: src\/view.tsx/u);
  });
});
