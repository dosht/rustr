#!/usr/bin/env node
import { readFile, writeFile, mkdir, realpath } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { argumentsOf, LOCALES, parseStrictJson, validateCatalogs } from './validate.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sameArguments = (a, b) => {
  const left = argumentsOf(a), right = argumentsOf(b);
  return left.size === right.size && [...left].every(name => right.has(name));
};
export function importCatalogs(capture, translations, config) {
  if (capture.version !== 1 || translations.version !== 1) throw new Error('Unsupported import data version');
  const reachable = capture.reachable;
  const catalogs = Object.create(null);
  const resolutions = [];
  for (const locale of LOCALES) {
    catalogs[locale] = Object.create(null);
    const shared = new Map();
    if (locale !== 'en') for (const [ns, source] of Object.entries(capture.catalogs[locale] ?? {})) {
      for (const [key, value] of Object.entries(source)) {
        const english = capture.catalogs.en[ns]?.[key];
        if (typeof value === 'string' && typeof english === 'string' && sameArguments(value, english) && !shared.has(english)) shared.set(english, { value, ns, key });
      }
    }
    for (const [ns, entries] of Object.entries(reachable.namespaces)) {
      if (!config.namespaces.includes(ns)) throw new Error(`Unconfigured namespace ${ns}`);
      const catalog = Object.create(null);
      for (const [key, entry] of Object.entries(entries)) {
        if (typeof entry.english !== 'string' || !entry.english.trim()) throw new Error(`Missing English meaning for ${ns}/${key}`);
        let value, origin;
        if (locale === 'en') { value = entry.english; origin = { kind: 'english_meaning' }; }
        else {
          const original = capture.catalogs[locale]?.[ns]?.[key];
          const correction = translations.overrides?.[locale]?.[ns]?.[key];
          if (correction !== undefined) {
            value = correction; origin = { kind: 'translated_source_correction' };
          } else if (typeof original === 'string' && sameArguments(original, entry.english)) {
            value = original; origin = { kind: 'source', ns, key };
          } else if (shared.has(entry.english)) {
            const reuse = shared.get(entry.english);
            value = reuse.value; origin = { kind: 'source_equivalent', ns: reuse.ns, key: reuse.key };
          } else {
            value = translations.messages?.[entry.english]?.[locale];
            origin = { kind: 'translated', english: entry.english };
          }
        }
        if (typeof value !== 'string' || !value.trim()) throw new Error(`Translation required: ${locale}/${ns}/${key}`);
        if (!sameArguments(value, entry.english)) throw new Error(`Translation arguments differ: ${locale}/${ns}/${key}`);
        catalog[key] = value;
        resolutions.push({ locale, ns, key, ...origin });
      }
      for (const key of Object.keys(entries)) {
        if (!key.endsWith('_other')) continue;
        const base = key.slice(0, -6);
        for (const category of new Intl.PluralRules(locale).resolvedOptions().pluralCategories) {
          const variant = `${base}_${category}`;
          if (Object.hasOwn(catalog, variant)) continue;
          // Every added CLDR category has an explicit authored value. Neither
          // English nor another grammatical category is an automatic fallback.
          const value = translations.pluralMessages?.[`${ns}/${variant}`]?.[locale];
          if (typeof value !== 'string' || !value.trim() || !sameArguments(value, entries[key].english)) throw new Error(`Explicit plural translation required: ${locale}/${ns}/${variant}`);
          catalog[variant] = value;
          resolutions.push({ locale, ns, key: variant, kind: 'translated_plural' });
        }
      }
      catalogs[locale][ns] = catalog;
    }
  }
  const fallback = { ...capture.fallback, active: [], policy: 'P5 D3: source baseline is evidence only; no runtime English fallback.' };
  const provenance = { ...capture.provenance, author: translations.author, decision: 'P5 D3', resolutions };
  const result = { config, reachable, fallback, provenance, catalogs };
  const issues = validateCatalogs(result);
  if (issues.length) throw new Error(`Catalog import rejected: ${JSON.stringify(issues)}`);
  return result;
}

async function main() {
  const options = Object.create(null);
  const args = process.argv.slice(2);
  for (let index = 0; index < args.length; index += 2) {
    if (!['--capture', '--root', '--translations'].includes(args[index]) || !args[index + 1] || options[args[index]]) throw new Error('usage: node scripts/p5-locales/import.mjs --capture <report.json> [--root <catalog-root>] [--translations <translations.json>]');
    options[args[index]] = args[index + 1];
  }
  if (!options['--capture']) throw new Error('--capture is required; import never guesses a source repository.');
  const root = await realpath(options['--root'] ?? fileURLToPath(new URL('../../transgate/web/locales/', import.meta.url)));
  const translationPath = options['--translations'] ?? fileURLToPath(new URL('./translations.json', import.meta.url));
  const readJson = async filename => parseStrictJson(await readFile(filename, 'utf8'), filename);
  const capture = await readJson(options['--capture']);
  const config = await readJson(path.join(root, 'config.json'));
  for (const [field, filename] of Object.entries({ reachabilityManifest: 'reachable-keys.json', fallbackManifest: 'fallback-manifest.json', provenanceManifest: 'source-provenance.json' })) {
    if (config[field] !== filename) throw new Error(`Invalid catalog metadata output path ${field}`);
  }
  const translationBytes = await readFile(translationPath);
  const translations = parseStrictJson(translationBytes.toString('utf8'), translationPath);
  const sourceRoot = await realpath(capture.provenance.sourceRoot);
  const underSource = candidate => { const rel = path.relative(sourceRoot, candidate); return rel === '' || rel !== '..' && !rel.startsWith(`..${path.sep}`) && !path.isAbsolute(rel); };
  if (underSource(root)) throw new Error('Import output must not be inside the read-only source repository.');
  const result = importCatalogs(capture, translations, config);
  result.provenance.translationInput = { path: translationPath, sha256: hash(translationBytes) };
  // Prepare every output and inspect directory symlinks before changing any file.
  const writes = [];
  for (const [locale, namespaces] of Object.entries(result.catalogs)) {
    const directory = path.join(root, locale);
    await mkdir(directory, { recursive: true });
    const actual = await realpath(directory);
    if (actual !== directory || underSource(actual)) throw new Error(`Refusing symlinked catalog directory ${locale}`);
    for (const [ns, value] of Object.entries(namespaces)) writes.push([path.join(directory, `${ns}.json`), value]);
  }
  writes.push([path.join(root, config.reachabilityManifest), result.reachable], [path.join(root, config.fallbackManifest), result.fallback], [path.join(root, config.provenanceManifest), result.provenance]);
  for (const [filename] of writes) {
    try { if (await realpath(filename) !== filename) throw new Error(`Refusing symlinked catalog output ${filename}`); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  for (const [filename, value] of writes) await writeFile(filename, `${JSON.stringify(value, null, 2)}\n`);
  console.log(JSON.stringify({ status: 'IMPORTED', locales: LOCALES.length, catalogs: LOCALES.length * config.namespaces.length }));
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch(error => { console.error(JSON.stringify({ status: 'FAILED', error: error.message })); process.exitCode = 1; });
}
