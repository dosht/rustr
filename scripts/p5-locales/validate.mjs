#!/usr/bin/env node
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const LOCALES = ['en', 'tr', 'pt', 'es', 'de', 'id', 'it', 'fr', 'el', 'ru', 'da', 'sv', 'no', 'fi', 'kk', 'az'];
const plain = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const sameSet = (a, b) => a.size === b.size && [...a].every(value => b.has(value));
const pluralSuffix = /^(.*)_(zero|one|two|few|many|other)$/u;

// JSON.parse alone silently accepts duplicate members. Reject them at every depth,
// including escaped spellings of the same key, before accepting catalog data.
export function parseStrictJson(text, filename = '<json>') {
  const parsed = JSON.parse(text);
  let offset = 0;
  const space = () => { while (/\s/u.test(text[offset] ?? '') && offset < text.length) offset++; };
  function string() {
    const start = offset++;
    while (offset < text.length) {
      const char = text[offset++];
      if (char === '\\') offset++;
      else if (char === '"') return JSON.parse(text.slice(start, offset));
    }
    throw new Error(`${filename}: unterminated string`);
  }
  function value(location) {
    space();
    if (text[offset] === '{') {
      offset++;
      const keys = new Set();
      space();
      if (text[offset] === '}') { offset++; return; }
      while (offset < text.length) {
        space();
        const key = string();
        if (keys.has(key)) throw new Error(`${filename}: duplicate key ${JSON.stringify(key)} at ${location}`);
        keys.add(key);
        space();
        offset++; // ':'; JSON.parse already established the grammar.
        value(`${location}/${key}`);
        space();
        if (text[offset++] === '}') return;
      }
    } else if (text[offset] === '[') {
      offset++;
      space();
      if (text[offset] === ']') { offset++; return; }
      let index = 0;
      while (offset < text.length) {
        value(`${location}/${index++}`);
        space();
        if (text[offset++] === ']') return;
      }
    } else if (text[offset] === '"') string();
    else while (offset < text.length && !/[\s,\]}]/u.test(text[offset])) offset++;
  }
  value('');
  return parsed;
}

export function argumentsOf(text) {
  if (typeof text !== 'string') return new Set();
  return new Set([...text.matchAll(/\{\{\s*-?\s*([^},\s]+)(?:\s*,[^}]*)?\s*\}\}/gu)].map(match => match[1]));
}

export function validateCatalogs({ config, reachable, catalogs, fallback, provenance }) {
  const issues = [];
  const issue = (code, locale, ns, key, message) => issues.push({ code, locale, ns, key, message });
  if (!plain(config) || !Array.isArray(config.locales) || config.locales.length !== LOCALES.length || !sameSet(new Set(config.locales), new Set(LOCALES))) {
    issue('locales', null, null, null, 'Exactly the 16 supported source UI locales are required.');
  }
  const namespaces = config?.namespaces ?? [];
  if (!namespaces.length || new Set(namespaces).size !== namespaces.length || namespaces.some(ns => !/^[a-z]+$/u.test(ns))) issue('namespaces', null, null, null, 'Namespaces must be unique explicit lowercase file stems.');
  const options = config?.i18next;
  for (const [key, expected] of Object.entries({ fallbackLng: false, keySeparator: false, nsSeparator: false, ignoreJSONStructure: false, pluralSeparator: '_', contextSeparator: '_', returnNull: false, returnEmptyString: false, returnObjects: false, saveMissing: false })) {
    if (options?.[key] !== expected) issue('resolver_option', null, null, key, `Expected ${JSON.stringify(expected)}.`);
  }
  if (options?.interpolation?.escapeValue !== false) issue('resolver_option', null, null, 'interpolation.escapeValue', 'Svelte escapes rendered text; i18next must not double-escape it.');
  if (fallback?.version !== 1 || !Array.isArray(fallback.baseline) || !Array.isArray(fallback.active)) issue('fallback_manifest', null, null, null, 'Expected version 1 baseline and active arrays.');
  else if (fallback.active.length) issue('active_fallback', null, null, null, 'P5 D3 requires translated reachable keys; baseline discovery never enables runtime fallback.');
  if (provenance?.version !== 1 || !Array.isArray(provenance.files) || !provenance.files.length) issue('provenance', null, null, null, 'Source paths and SHA-256 provenance are required.');
  else for (const file of provenance.files) if (typeof file.path !== 'string' || !/^[a-f0-9]{64}$/u.test(file.sha256 ?? '')) issue('provenance', null, null, file.path ?? null, 'Invalid source hash record.');
  const entries = reachable?.namespaces;
  if (reachable?.version !== 1 || !plain(entries)) { issue('reachability', null, null, null, 'Missing version 1 namespace/key inventory.'); return issues; }
  if (!sameSet(new Set(Object.keys(entries)), new Set(namespaces))) issue('namespace_inventory', null, null, null, 'Reachability and configured namespaces differ.');
  for (const locale of Object.keys(catalogs ?? {})) if (!LOCALES.includes(locale)) issue('unexpected_locale', locale, null, null, 'Locale is outside the product contract.');
  for (const locale of LOCALES) {
    for (const ns of Object.keys(catalogs?.[locale] ?? {})) if (!namespaces.includes(ns)) issue('unexpected_namespace', locale, ns, null, 'Catalog namespace is outside the P5 contract.');
    for (const ns of namespaces) {
      const catalog = catalogs?.[locale]?.[ns];
      if (!plain(catalog)) { issue('missing_catalog', locale, ns, null, 'Expected a flat JSON string map.'); continue; }
      for (const [key, text] of Object.entries(catalog)) {
        if (typeof text !== 'string' || !text.trim()) { issue('invalid_value', locale, ns, key, 'Catalog values must be nonempty strings.'); continue; }
        if (key === '__proto__' || key === 'constructor' || key === 'prototype') issue('unsafe_key', locale, ns, key, 'Reserved object key.');
        const stripped = text.replace(/\{\{\s*-?\s*([^},\s]+)(?:\s*,[^}]*)?\s*\}\}/gu, '');
        if (stripped.includes('{{') || stripped.includes('}}')) issue('malformed_interpolation', locale, ns, key, 'Unbalanced i18next interpolation.');
      }
      for (const [key, entry] of Object.entries(entries[ns] ?? {})) {
        const text = catalog[key];
        if (!Object.hasOwn(catalog, key)) { issue('missing_key', locale, ns, key, 'Reachable key has no locale value.'); continue; }
        if (typeof entry.english !== 'string' || !entry.english || !Array.isArray(entry.arguments) || !Array.isArray(entry.sources) || !entry.sources.length) {
          issue('invalid_inventory_entry', locale, ns, key, 'Expected English meaning, arguments and source references.'); continue;
        }
        if (!sameSet(argumentsOf(entry.english), new Set(entry.arguments))) issue('inventory_arguments', locale, ns, key, 'Declared argument names disagree with the English message.');
        if (locale === 'en' && text !== entry.english) issue('english_anchor', locale, ns, key, 'English catalog differs from the declared source/approved meaning.');
        if (typeof text === 'string' && !sameSet(argumentsOf(text), argumentsOf(entry.english))) issue('interpolation', locale, ns, key, 'Interpolation variable names changed or disappeared.');
      }
      // Groups are explicit i18next v4 keys. No synthetic English initializer is
      // used for categories absent from a locale's source resource.
      const groups = new Map();
      for (const key of Object.keys(entries[ns] ?? {})) {
        const match = key.match(pluralSuffix);
        if (match) { if (!groups.has(match[1])) groups.set(match[1], new Set()); groups.get(match[1]).add(match[2]); }
      }
      for (const [base, suffixes] of groups) {
        if (!suffixes.has('other')) continue; // A natural source key, not a plural family.
        const categories = new Intl.PluralRules(locale).resolvedOptions().pluralCategories;
        for (const category of categories) {
          const key = `${base}_${category}`;
          if (!Object.hasOwn(catalog, key)) issue('plural_category', locale, ns, key, `Missing ${locale} plural category ${category}.`);
          else {
            const reference = entries[ns][key] ?? entries[ns][`${base}_other`];
            if (typeof catalog[key] === 'string' && !sameSet(argumentsOf(catalog[key]), argumentsOf(reference.english))) issue('plural_arguments', locale, ns, key, 'Plural variant interpolation differs from its reference.');
          }
        }
      }
    }
  }
  return issues;
}

export async function loadCatalogs(root) {
  const load = async relative => parseStrictJson(await readFile(path.join(root, relative), 'utf8'), relative);
  const config = await load('config.json');
  for (const [field, filename] of Object.entries({ reachabilityManifest: 'reachable-keys.json', fallbackManifest: 'fallback-manifest.json', provenanceManifest: 'source-provenance.json' })) {
    if (config[field] !== filename) throw new Error(`Invalid catalog metadata path ${field}`);
  }
  const reachable = await load(config.reachabilityManifest);
  const fallback = await load(config.fallbackManifest);
  const provenance = await load(config.provenanceManifest);
  const catalogs = Object.create(null);
  const directories = (await readdir(root, { withFileTypes: true })).filter(entry => entry.isDirectory());
  for (const directory of directories) {
    const locale = directory.name;
    catalogs[locale] = Object.create(null);
    for (const file of await readdir(path.join(root, locale))) {
      if (!file.endsWith('.json')) throw new Error(`Unexpected catalog asset ${locale}/${file}`);
      const ns = file.slice(0, -5);
      catalogs[locale][ns] = await load(`${locale}/${file}`);
    }
  }
  return { config, reachable, fallback, provenance, catalogs };
}

async function main() {
  const args = process.argv.slice(2);
  const root = args.length === 0 ? fileURLToPath(new URL('../../transgate/web/locales/', import.meta.url))
    : args.length === 2 && args[0] === '--root' ? path.resolve(args[1]) : null;
  if (!root) throw new Error('usage: node scripts/p5-locales/validate.mjs [--root <locales-dir>]');
  const input = await loadCatalogs(root);
  const issues = validateCatalogs(input);
  console.log(JSON.stringify({ status: issues.length ? 'FAILED' : 'PASSED', locales: LOCALES.length, issues }, null, 2));
  if (issues.length) process.exitCode = 1;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch(error => { console.error(JSON.stringify({ status: 'FAILED', error: error.message })); process.exitCode = 1; });
}
