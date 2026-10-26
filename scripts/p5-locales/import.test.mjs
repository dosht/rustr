import test from 'node:test';
import assert from 'node:assert/strict';
import { importCatalogs } from './import.mjs';
import { LOCALES } from './validate.mjs';

function fixture() {
  const labels = { en: 'Email address', tr: 'E-posta adresi', pt: 'Endereço de e-mail', es: 'Dirección de correo electrónico', de: 'E-Mail-Adresse', id: 'Alamat email', it: 'Indirizzo email', fr: 'Adresse e-mail', el: 'Διεύθυνση email', ru: 'Адрес электронной почты', da: 'E-mailadresse', sv: 'E-postadress', no: 'E-postadresse', fi: 'Sähköpostiosoite', kk: 'Электрондық пошта мекенжайы', az: 'E-poçt ünvanı' };
  return {
    config: { locales: LOCALES, namespaces: ['auth'], i18next: { fallbackLng: false, keySeparator: false, nsSeparator: false, ignoreJSONStructure: false, pluralSeparator: '_', contextSeparator: '_', returnNull: false, returnEmptyString: false, returnObjects: false, saveMissing: false, interpolation: { escapeValue: false } } },
    capture: { version: 1, catalogs: Object.fromEntries(LOCALES.map(locale => [locale, { auth: { 'sign_in.email_label': labels[locale] } }])),
      reachable: { version: 1, namespaces: { auth: { 'sign_in.email_label': { english: labels.en, arguments: [], sources: ['synthetic/auth.tsx:1'] } } } },
      fallback: { version: 1, baseline: [], active: [] }, provenance: { version: 1, files: [{ path: 'synthetic/auth.tsx', sha256: '0'.repeat(64) }] } },
    translations: { version: 1, messages: {}, pluralMessages: {} },
  };
}

test('a missing non-English value aborts import instead of initializing from English', () => {
  const { config, capture, translations } = fixture();
  delete capture.catalogs.kk.auth['sign_in.email_label'];
  assert.throws(() => importCatalogs(capture, translations, config), /Translation required: kk\/auth\/sign_in.email_label/u);
  assert.equal(Object.hasOwn(capture.catalogs.kk.auth, 'sign_in.email_label'), false);
});

test('source interpolation corruption needs a real replacement preserving the protocol argument', () => {
  const { config, capture, translations } = fixture();
  for (const locale of LOCALES) capture.catalogs[locale].auth.provider = `${locale} {{provider}}`;
  capture.catalogs.en.auth.provider = 'Continue with {{provider}}';
  capture.catalogs.tr.auth.provider = '{{sağlayıcı}} ile devam et';
  capture.reachable.namespaces.auth.provider = { english: 'Continue with {{provider}}', arguments: ['provider'], sources: ['synthetic/auth.tsx:2'] };
  assert.throws(() => importCatalogs(capture, translations, config), /Translation required: tr\/auth\/provider/u);
  translations.messages['Continue with {{provider}}'] = { tr: '{{provider}} ile devam et' };
  const result = importCatalogs(capture, translations, config);
  assert.equal(result.catalogs.tr.auth.provider, '{{provider}} ile devam et');
  assert.equal(capture.catalogs.tr.auth.provider, '{{sağlayıcı}} ile devam et');
});

test('an authored translation cannot silently rename interpolation variables', () => {
  const { config, capture, translations } = fixture();
  capture.reachable.namespaces.auth.greeting = { english: 'Hello {{name}}', arguments: ['name'], sources: ['synthetic/auth.tsx:3'] };
  translations.messages['Hello {{name}}'] = {};
  translations.messages['Hello {{name}}'].tr = 'Merhaba {{isim}}';
  assert.throws(() => importCatalogs(capture, translations, config), /Translation arguments differ: tr\/auth\/greeting/u);
});

test('adding an unconfigured P6 namespace cannot expand the importer output', () => {
  const { config, capture, translations } = fixture();
  capture.reachable.namespaces.tools = { recorder: { english: 'Recorder', arguments: [], sources: ['synthetic/public.tsx:1'] } };
  assert.throws(() => importCatalogs(capture, translations, config), /Unconfigured namespace tools/u);
});
