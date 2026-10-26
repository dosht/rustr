#!/usr/bin/env node
// Static capture only: never import/transpile/execute the source application.
import { readFile, realpath, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { argumentsOf, LOCALES, parseStrictJson } from './validate.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const inside = (root, candidate) => { const rel = path.relative(root, candidate); return rel !== '..' && !rel.startsWith(`..${path.sep}`) && !path.isAbsolute(rel); };
export function flattenStrings(object, prefix = '', result = Object.create(null)) {
  if (!object || typeof object !== 'object' || Array.isArray(object)) throw new Error(`Not a resource object at ${prefix}`);
  for (const [name, value] of Object.entries(object)) {
    const key = prefix ? `${prefix}.${name}` : name;
    if (typeof value === 'string') {
      if (Object.hasOwn(result, key)) throw new Error(`Ambiguous flat resource key ${key}`);
      result[key] = value;
    } else if (value && typeof value === 'object' && !Array.isArray(value)) flattenStrings(value, key, result);
    else throw new Error(`Non-string resource leaf ${key}`);
  }
  return result;
}

// The six malformed source quotation delimiters are outside the P5 UI, but the
// whole resource must parse before selecting its reachable P5 members. Do not
// silently repair any other file or future syntax error.
export function repairGermanApp(text) {
  const eol = text.includes('\r\n') ? '\r\n' : '\n';
  const lines = text.split(eol);
  const repairs = [];
  for (const line of [115, 118, 121, 124, 127, 130]) {
    const before = lines[line - 1];
    if (!before?.startsWith('  "\\"') || !before.includes(': "„') || !before.endsWith('."",')) {
      throw new Error(`German source changed: expected the recorded malformed quotation at line ${line}`);
    }
    lines[line - 1] = `${before.slice(0, -3)}“",`;
    repairs.push({ path: 'public/locales/de/app.json', line, before, after: lines[line - 1], decision: 'P5 D3' });
  }
  return { text: lines.join(eol), repairs };
}

export function extractAuth(ts, text) {
  const ast = ts.createSourceFile('signin.tsx', text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  function literal(node) {
    if (ts.isStringLiteralLike(node)) return node.text;
    if (!ts.isObjectLiteralExpression(node)) throw new Error('Auth labels contain a non-literal expression');
    const object = Object.create(null);
    for (const property of node.properties) {
      if (!ts.isPropertyAssignment(property) || !('text' in property.name)) throw new Error('Auth labels contain a computed/spread property');
      if (Object.hasOwn(object, property.name.text)) throw new Error(`Duplicate auth label ${property.name.text}`);
      object[property.name.text] = literal(property.initializer);
    }
    return object;
  }
  let result;
  function visit(node) {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text === 'locales') {
      if (result) throw new Error('Ambiguous auth locales object');
      result = literal(node.initializer);
    }
    ts.forEachChild(node, visit);
  }
  visit(ast);
  if (!result || LOCALES.some(locale => !result[locale])) throw new Error('The source auth object must contain all 16 locales');
  return Object.fromEntries(LOCALES.map(locale => [locale, flattenStrings(result[locale])]));
}

export function extractKeys(ts, text, filename, english, excludedTags = ['Head']) {
  const ast = ts.createSourceFile(filename, text, ts.ScriptTarget.Latest, true, filename.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const aliases = new Map();
  const keys = new Map();
  const dynamic = [];
  const literals = [];
  const sourceAt = node => `${filename}:${ast.getLineAndCharacterOfPosition(node.getStart(ast)).line + 1}`;
  function add(ns, key, node, fallback) {
    const id = JSON.stringify([ns, key]);
    if (!keys.has(id)) keys.set(id, { ns, key, english: english[ns]?.[key] ?? fallback, sources: [] });
    const sources = keys.get(id).sources;
    const source = sourceAt(node);
    if (!sources.includes(source)) sources.push(source);
  }
  function bindings(node) {
    if (ts.isVariableDeclaration(node) && node.initializer && ts.isCallExpression(node.initializer)
      && node.initializer.expression.getText(ast) === 'useTranslation' && ts.isObjectBindingPattern(node.name)) {
      const argument = node.initializer.arguments[0];
      const ns = argument && ts.isStringLiteralLike(argument) ? argument.text : 'common';
      for (const element of node.name.elements) if ((element.propertyName?.getText(ast) ?? element.name.getText(ast)) === 't') aliases.set(element.name.getText(ast), ns);
    }
    ts.forEachChild(node, bindings);
  }
  bindings(ast);
  function visit(node) {
    if (ts.isJsxElement(node) && excludedTags.includes(node.openingElement.tagName.getText(ast))) return;
    if (ts.isStringLiteralLike(node)) for (const [ns, resource] of Object.entries(english)) if (Object.hasOwn(resource, node.text)) add(ns, node.text, node);
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && aliases.has(node.expression.text)) {
      const ns = aliases.get(node.expression.text);
      const argument = node.arguments[0];
      if (argument && ts.isStringLiteralLike(argument)) {
        const key = argument.text;
        const variants = Object.keys(english[ns] ?? {}).filter(candidate => candidate === key || ['zero', 'one', 'two', 'few', 'many', 'other'].some(suffix => candidate === `${key}_${suffix}`));
        if (variants.length) for (const variant of variants) add(ns, variant, node);
        else dynamic.push({ ns, key, source: sourceAt(node), reason: 'source key absent from English JSON; explicit meaning required' });
      } else dynamic.push({ ns, expression: argument?.getText(ast), source: sourceAt(node), reason: 'dynamic source key; explicit domain mapping required' });
    }
    if (ts.isJsxText(node)) {
      const value = node.text.replace(/\s+/gu, ' ').trim();
      if (/[A-Za-z]{2}/u.test(value)) literals.push({ text: value, source: sourceAt(node) });
    }
    ts.forEachChild(node, visit);
  }
  visit(ast);
  return { keys: [...keys.values()], dynamic, literals };
}

export async function captureSource({ sourceRoot, scope, typescript }) {
  const root = await realpath(sourceRoot);
  const files = new Map();
  const repairs = [];
  const duplicates = [];
  async function read(relative) {
    if (path.isAbsolute(relative) || relative.split(/[\\/]/u).includes('..') || !/^(src\/|public\/locales\/)/u.test(relative)) throw new Error(`Source path outside capture scope: ${relative}`);
    const filename = await realpath(path.join(root, relative));
    if (!inside(root, filename)) throw new Error(`Source symlink leaves root: ${relative}`);
    const bytes = await readFile(filename);
    const sha256 = hash(bytes);
    if (scope.sourceHashes?.[relative] && scope.sourceHashes[relative] !== sha256) throw new Error(`Source snapshot changed: ${relative}`);
    files.set(relative, { path: relative, sha256 });
    return bytes.toString('utf8');
  }
  const ts = typescript ?? createRequire(path.join(root, 'package.json'))('typescript');
  function parseSource(text, filename) {
    const parsed = JSON.parse(text); // Source semantics are last-wins; record duplicates below.
    const ast = ts.createSourceFile(filename, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.JSON);
    function visit(node, prefix = '') {
      if (ts.isObjectLiteralExpression(node)) {
        const seen = new Set();
        for (const property of node.properties) if (ts.isPropertyAssignment(property) && 'text' in property.name) {
          const key = property.name.text;
          if (seen.has(key)) duplicates.push({ path: filename, key: prefix ? `${prefix}.${key}` : key });
          seen.add(key);
          visit(property.initializer, prefix ? `${prefix}.${key}` : key);
        }
      } else ts.forEachChild(node, child => visit(child, prefix));
    }
    visit(ast);
    return flattenStrings(parsed);
  }
  const catalogs = Object.create(null);
  for (const locale of LOCALES) {
    catalogs[locale] = Object.create(null);
    for (const ns of scope.namespaces) {
      if (!/^[a-z]+$/u.test(ns)) throw new Error(`Invalid namespace ${ns}`);
      const filename = `public/locales/${locale}/${ns}.json`;
      let text = await read(filename);
      if (locale === 'de' && ns === 'app') { const repaired = repairGermanApp(text); text = repaired.text; repairs.push(...repaired.repairs); }
      catalogs[locale][ns] = parseSource(text, filename);
    }
  }
  const english = catalogs.en;
  const auth = extractAuth(ts, await read('src/pages/signin.tsx'));
  for (const locale of LOCALES) catalogs[locale].auth = auth[locale];
  const reachable = { version: 1, namespaces: Object.fromEntries([...scope.namespaces, 'auth'].map(ns => [ns, Object.create(null)])) };
  const dynamic = [];
  const literals = [];
  for (const filename of scope.entrypoints) {
    if ((scope.excludePaths ?? []).includes(filename)) continue;
    if (!filename.startsWith('src/') || /(?:\.stories\.|\.test\.|__tests__)/u.test(filename)) throw new Error(`Non-product entrypoint ${filename}`);
    const extracted = extractKeys(ts, await read(filename), filename, english, scope.excludeJsxTags);
    dynamic.push(...extracted.dynamic);
    literals.push(...extracted.literals);
    for (const entry of extracted.keys) {
      const namespace = reachable.namespaces[entry.ns];
      if (!namespace) throw new Error(`Source uses an unconfigured namespace ${entry.ns}`);
      const current = namespace[entry.key];
      if (current) current.sources = [...new Set([...current.sources, ...entry.sources])];
      else namespace[entry.key] = { english: entry.english, arguments: [...argumentsOf(entry.english)], sources: entry.sources };
    }
  }
  for (const [key, text] of Object.entries(auth.en)) reachable.namespaces.auth[key] = { english: text, arguments: [...argumentsOf(text)], sources: ['src/pages/signin.tsx:244-1157'] };
  // New approved copy and hard-coded source labels are explicit, never guessed
  // from a semantic key or initialized from English into a foreign catalog.
  for (const [ns, entries] of Object.entries(scope.extraKeys ?? {})) for (const [key, entry] of Object.entries(entries)) {
    if (!reachable.namespaces[ns] || typeof entry.english !== 'string' || !Array.isArray(entry.sources) || !entry.sources.length) throw new Error(`Invalid explicit key ${ns}/${key}`);
    reachable.namespaces[ns][key] = { english: entry.english, arguments: [...argumentsOf(entry.english)], sources: entry.sources };
  }
  const baseline = [];
  const untranslated = new Set((scope.untranslatedSourceKeys ?? []).map(({locale, ns, key}) => JSON.stringify([locale, ns, key])));
  for (const locale of LOCALES.filter(value => value !== 'en')) for (const [ns, entries] of Object.entries(reachable.namespaces)) for (const key of Object.keys(entries)) {
    if (scope.extraKeys?.[ns]?.[key]?.origin === 'approved') continue;
    const original = catalogs[locale][ns][key];
    const expectedArgs = argumentsOf(entries[key].english);
    const sourceArgs = argumentsOf(original);
    const reason = locale === 'de' && ns === 'app' ? 'invalid_source_namespace'
      : original === undefined ? Object.hasOwn(catalogs.en[ns], key) ? 'missing_source_key' : 'hardcoded_source_copy'
      : typeof original === 'string' && !original.trim() ? 'source_empty_value'
      : expectedArgs.size !== sourceArgs.size || [...expectedArgs].some(name => !sourceArgs.has(name)) ? 'source_placeholder_mismatch'
      : untranslated.has(JSON.stringify([locale, ns, key])) ? 'source_untranslated_copy' : null;
    if (reason) baseline.push({ locale, ns, key, reason, source: ns === 'auth' ? 'src/pages/signin.tsx' : `public/locales/${locale}/${ns}.json` });
  }
  return { version: 1, catalogs, reachable, fallback: { version: 1, baseline, active: [] },
    provenance: { version: 1, sourceRoot: root, typescriptVersion: ts.version, files: [...files.values()], repairs, duplicates }, dynamic, literals };
}

async function main() {
  const options = Object.create(null);
  const args = process.argv.slice(2);
  for (let index = 0; index < args.length; index += 2) {
    if (!['--source-root', '--scope', '--output'].includes(args[index]) || !args[index + 1] || options[args[index]]) throw new Error('usage: node scripts/p5-locales/capture.mjs --source-root <frontend> --scope <scope.json> --output <report.json>');
    options[args[index]] = args[index + 1];
  }
  if (!options['--source-root'] || !options['--scope'] || !options['--output']) throw new Error('All three capture options are required.');
  const root = await realpath(options['--source-root']);
  const output = path.resolve(options['--output']);
  const parent = await realpath(path.dirname(output));
  if (inside(root, path.join(parent, path.basename(output)))) throw new Error('Capture output must not be inside the read-only source repository.');
  try { if (inside(root, await realpath(output))) throw new Error('Capture output symlink targets the read-only source repository.'); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  const scope = parseStrictJson(await readFile(options['--scope'], 'utf8'), options['--scope']);
  const report = await captureSource({ sourceRoot: root, scope });
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
  console.log(JSON.stringify({ status: 'CAPTURED', output, files: report.provenance.files.length, dynamic: report.dynamic.length }));
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch(error => { console.error(JSON.stringify({ status: 'FAILED', error: error.message })); process.exitCode = 1; });
}
