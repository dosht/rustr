import test from 'node:test';
import assert from 'node:assert/strict';
import { argumentsOf, LOCALES, parseStrictJson, validateCatalogs } from './validate.mjs';

const labels = {
  en: 'Email address', tr: 'E-posta adresi', pt: 'Endereço de e-mail', es: 'Dirección de correo electrónico',
  de: 'E-Mail-Adresse', id: 'Alamat email', it: 'Indirizzo email', fr: 'Adresse e-mail', el: 'Διεύθυνση email',
  ru: 'Адрес электронной почты', da: 'E-mailadresse', sv: 'E-postadress', no: 'E-postadresse',
  fi: 'Sähköpostiosoite', kk: 'Электрондық пошта мекенжайы', az: 'E-poçt ünvanı',
};
function fixture() {
  return {
    config: {
      locales: LOCALES, namespaces: ['auth'],
      i18next: { fallbackLng: false, keySeparator: false, nsSeparator: false, ignoreJSONStructure: false,
        pluralSeparator: '_', contextSeparator: '_', returnNull: false, returnEmptyString: false,
        returnObjects: false, saveMissing: false, interpolation: { escapeValue: false } },
    },
    reachable: { version: 1, namespaces: { auth: {
      'sign_in.email_label': { english: labels.en, arguments: [], sources: ['synthetic/auth.tsx:1'] },
    } } },
    catalogs: Object.fromEntries(LOCALES.map(locale => [locale, { auth: { 'sign_in.email_label': labels[locale] } }])),
    fallback: { version: 1, baseline: [{ locale: 'kk', ns: 'auth', key: 'sign_in.email_label', reason: 'missing_source_key', source: 'synthetic' }], active: [] },
    provenance: { version: 1, files: [{ path: 'synthetic/auth.tsx', sha256: '0'.repeat(64) }] },
  };
}
const codes = input => validateCatalogs(input).map(({ code, locale, ns, key }) => ({ code, locale, ns, key }));

test('a missing supported-locale entry cannot be accepted through its source fallback inventory', () => {
  const input = fixture();
  delete input.catalogs.kk.auth['sign_in.email_label'];
  assert.deepEqual(codes(input), [{ code: 'missing_key', locale: 'kk', ns: 'auth', key: 'sign_in.email_label' }]);
});

test('runtime English fallback remains forbidden after D3 even for a baseline gap', () => {
  const input = fixture();
  input.fallback.active.push(input.fallback.baseline[0]);
  assert.deepEqual(codes(input), [{ code: 'active_fallback', locale: null, ns: null, key: null }]);
});

test('literal dot and colon keys are not mistaken for nested objects or namespaces', () => {
  const input = fixture();
  const entry = { english: 'Email address', arguments: [], sources: ['synthetic/auth.tsx:2'] };
  input.reachable.namespaces.auth['label:email.address'] = entry;
  for (const locale of LOCALES) input.catalogs[locale].auth['label:email.address'] = labels[locale];
  delete input.catalogs.fr.auth['label:email.address'];
  input.catalogs.fr.auth.label = { 'email.address': labels.fr };
  const result = codes(input);
  assert.ok(result.some(issue => issue.code === 'missing_key' && issue.locale === 'fr' && issue.key === 'label:email.address'));
  assert.ok(result.some(issue => issue.code === 'invalid_value' && issue.locale === 'fr' && issue.key === 'label'));
});

test('renaming or dropping interpolation variables is a locale contract failure', () => {
  const input = fixture();
  const key = 'welcome';
  input.reachable.namespaces.auth[key] = { english: 'Welcome, {{name}}', arguments: ['name'], sources: ['synthetic/auth.tsx:3'] };
  for (const locale of LOCALES) input.catalogs[locale].auth[key] = `${labels[locale]} {{name}}`;
  input.catalogs.en.auth[key] = 'Welcome, {{name}}';
  input.catalogs.tr.auth[key] = 'Hoş geldiniz, {{ad}}';
  input.catalogs.de.auth[key] = 'Willkommen';
  assert.deepEqual(codes(input), [
    { code: 'interpolation', locale: 'tr', ns: 'auth', key },
    { code: 'interpolation', locale: 'de', ns: 'auth', key },
  ]);
  assert.deepEqual([...argumentsOf('{{- name}} / {{count, number}} / {{name}}')].sort(), ['count', 'name']);
});

test('Russian few and many categories cannot disappear behind English one/other variants', () => {
  const input = fixture();
  const en = { items_one: '{{count}} item', items_other: '{{count}} items' };
  for (const [key, english] of Object.entries(en)) input.reachable.namespaces.auth[key] = { english, arguments: ['count'], sources: ['synthetic/auth.tsx:4'] };
  for (const locale of LOCALES) for (const category of new Intl.PluralRules(locale).resolvedOptions().pluralCategories) {
    input.catalogs[locale].auth[`items_${category}`] = `${labels[locale]} {{count}}`;
  }
  // English source keys remain explicit inventory members even where CLDR has only other.
  for (const locale of LOCALES) for (const key of Object.keys(en)) input.catalogs[locale].auth[key] ??= `${labels[locale]} {{count}}`;
  Object.assign(input.catalogs.en.auth, en);
  input.catalogs.ru.auth.items_one = '{{count}} элемент';
  input.catalogs.ru.auth.items_other = '{{count}} элемента';
  delete input.catalogs.ru.auth.items_few;
  delete input.catalogs.ru.auth.items_many;
  assert.deepEqual(codes(input), [
    { code: 'plural_category', locale: 'ru', ns: 'auth', key: 'items_few' },
    { code: 'plural_category', locale: 'ru', ns: 'auth', key: 'items_many' },
  ]);
});

test('malformed interpolation is not accepted as untranslated literal copy', () => {
  const input = fixture();
  input.catalogs.fi.auth['sign_in.email_label'] = 'Sähköposti {{name';
  assert.ok(codes(input).some(issue => issue.code === 'malformed_interpolation' && issue.locale === 'fi'));
});

test('JSON duplicate keys including escaped aliases and nested keys fail instead of last-wins', () => {
  assert.throws(() => parseStrictJson('{"email":"a","\\u0065mail":"b"}'), /duplicate key/u);
  assert.throws(() => parseStrictJson('{"auth":{"label":"a","label":"b"}}'), /duplicate key/u);
  assert.throws(() => parseStrictJson('{"email":"unfinished}'), SyntaxError);
  assert.deepEqual(parseStrictJson('{"a":{"label":"a"},"b":{"label":"b"},"list":[{"label":"c"}]}'), {
    a: { label: 'a' }, b: { label: 'b' }, list: [{ label: 'c' }],
  });
});

test('a P6-only namespace or unsupported locale is not silently bundled', () => {
  const input = fixture();
  input.catalogs.en.tools = { recorder: 'Recorder' };
  input.catalogs.ja = { auth: { 'sign_in.email_label': 'メールアドレス' } };
  assert.deepEqual(codes(input), [
    { code: 'unexpected_locale', locale: 'ja', ns: null, key: null },
    { code: 'unexpected_namespace', locale: 'en', ns: 'tools', key: null },
  ]);
});

test('duplicate locale entries do not satisfy the exact 16-locale contract', () => {
  const input = fixture();
  input.config.locales = [...LOCALES, 'en'];
  assert.deepEqual(codes(input), [{ code: 'locales', locale: null, ns: null, key: null }]);
});
