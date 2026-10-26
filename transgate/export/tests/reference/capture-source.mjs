// Lead/integration-only reference acquisition. Never imported by production.
// Runs the actual read-only source exporters on the shared synthetic cases.
// No source file is edited. Only save/fetch mechanics are intercepted; no new
// document formatting implementation is present here.
import { createRequire } from 'node:module';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';

const here = dirname(fileURLToPath(import.meta.url));
const frontend = resolve(process.env.TRANSGATE_SOURCE_FRONTEND || '/home/mu/Business/Transgate/frontend');
const destination = process.env.TRANSGATE_DOCUMENT_REFERENCE_DIR;
if (!destination) throw new Error('Set TRANSGATE_DOCUMENT_REFERENCE_DIR to an owned capture directory');
if (process.env.TZ !== 'UTC') throw new Error('Run with TZ=UTC: fixture click-time is the UTC epoch');
const out = resolve(destination);
if (out === frontend || out.startsWith(frontend + '/')) throw new Error('The source repository is read-only');
const require = createRequire(join(frontend, 'package.json'));
const esbuild = require('esbuild');
const ts = require('typescript');
// Keep the real installed libraries in their native modules: bundling fontkit
// or the PDF renderer would break their own relative resource paths. Only
// source TypeScript and the save adapters are bundled below.
const packageExports = {
  docx: ['Document','Packer','Paragraph','TextRun','HeadingLevel','AlignmentType','BorderStyle'],
  '@react-pdf/renderer': ['Document','Page','View','Text','Image','StyleSheet','pdf'],
  'date-fns': ['format'],
};
globalThis.__transgatePackages = {};
for (const name of [...Object.keys(packageExports), 'jspdf', 'react']) {
  const native = await import(pathToFileURL(require.resolve(name)).href);
  globalThis.__transgatePackages[name] = native;
  for (const symbol of packageExports[name] || []) {
    if (native[symbol] === undefined && native.default?.[symbol] === undefined) throw new Error(`Source dependency export changed: ${name}.${symbol}`);
  }
}
const casesPath = resolve(here, '../../../core/src/documents/fixtures/analysis.json');
const cases = JSON.parse(await readFile(casesPath, 'utf8'));
const exporter = join(frontend, 'src/components/AIAnalysis/export');
const utils = join(frontend, 'src/lib/ai/analysis-utils.ts');
const billing = join(frontend, 'src/pages/billing/index.tsx');
const font = await readFile(resolve(here, '../../fonts/NotoSansArabic.ttf'));
const originalFetch = globalThis.fetch;
globalThis.fetch = async url => {
  if (String(url).includes('notosansarabic') || String(url).includes('notosansarabic/')) return new Response(font, {status: 200});
  throw new Error(`Reference capture refuses external I/O: ${url}`);
};
let active;
let pending = [];
globalThis.__transgateCapture = (filename, bytes) => { pending.push(writeFile(join(out, `${active}.${filename.split('.').pop()}`), bytes)); };
globalThis.__transgateBlob = (blob, filename) => { pending.push(blob.arrayBuffer().then(bytes => globalThis.__transgateCapture(filename, new Uint8Array(bytes)))); };
globalThis.__transgateInvoiceLogo = join(frontend, 'public/assets/Light-Logo-2x-100.jpg');

const billingSource = await readFile(billing, 'utf8');
const ast = ts.createSourceFile(billing, billingSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const wanted = new Set(['styles', 'InvoicePDF', 'generateInvoiceId']);
const invoiceDeclarations = ast.statements.filter(node => ts.isVariableStatement(node) && node.declarationList.declarations.some(d => wanted.has(d.name.getText(ast)))).map(n => n.getText(ast));
if (invoiceDeclarations.length !== 3) throw new Error('Source invoice declaration boundary changed; inspect it rather than inventing a reference');
const invoiceSource = invoiceDeclarations.join('\n');
if (!invoiceSource.includes('src="/assets/Light-Logo-2x-100.jpg"')) throw new Error('Source invoice logo boundary changed; do not capture a missing-image reference');
const invoiceModule = `import React from 'react';\nimport {Document,Page,View,Text,Image,StyleSheet,pdf} from '@react-pdf/renderer';\nimport {format} from 'date-fns';\n${invoiceSource.replace('src="/assets/Light-Logo-2x-100.jpg"', 'src={globalThis.__transgateInvoiceLogo}')}\nexport async function invoice(input) {return new Uint8Array(await (await pdf(React.createElement(InvoicePDF,input)).toBlob()).arrayBuffer());}`;
const result = await esbuild.build({
  stdin: {contents: `export {exportAsDocx} from ${JSON.stringify(join(exporter, 'docx-exporter.ts'))};\nexport {exportAsPdf} from ${JSON.stringify(join(exporter, 'pdf-exporter.ts'))};\nexport {formatFullAnalysisAsText,formatFullAnalysisAsHtml} from ${JSON.stringify(utils)};\nexport {invoice} from 'source-invoice';`, resolveDir: frontend, loader: 'ts'},
  bundle: true, write: false, platform: 'node', format: 'cjs', target: 'node22',
  plugins: [{name: 'reference-save-and-assets-only', setup(build) {
    build.onResolve({filter: /^file-saver$/}, () => ({path: 'capture-save', namespace: 'capture'}));
    build.onResolve({filter: /^source-invoice$/}, () => ({path: 'invoice', namespace: 'capture'}));
    build.onResolve({filter: /^(docx|jspdf|react|@react-pdf\/renderer|date-fns)$/}, args => ({path: args.path, namespace: 'source-native-library'}));
    build.onResolve({filter: /^html2canvas$/}, args => ({path: require.resolve(args.path), external: true}));
    build.onLoad({filter: /.*/, namespace: 'source-native-library'}, args => {
      const preamble = `const p=globalThis.__transgatePackages[${JSON.stringify(args.path)}];`;
      if (args.path === 'react') return {contents: `${preamble} export default (p.default || p);`, loader:'js'};
      if (args.path === 'jspdf') return {contents: `${preamble} const constructor=typeof p.default==='function'?p.default:(p.jsPDF || p.default?.jsPDF); if(typeof constructor!=='function') throw new Error('Source jsPDF export changed'); export default constructor;`, loader:'js'};
      return {contents: preamble + packageExports[args.path].map(name => `export const ${name}=p.${name} ?? p.default?.${name};`).join('\n'), loader:'js'};
    });
    build.onLoad({filter: /.*/, namespace: 'capture'}, args => args.path === 'capture-save'
      ? {contents: 'export function saveAs(blob,name){globalThis.__transgateBlob(blob,name)}', loader: 'js'}
      : {contents: invoiceModule, loader: 'tsx', resolveDir: frontend});
    build.onLoad({filter: /pdf-exporter\.ts$/}, async args => {
      const original = await readFile(args.path, 'utf8');
      if (!original.includes('pdf.save(filename)')) throw new Error('Source PDF save boundary changed');
      return {contents: original.replaceAll('pdf.save(filename)', 'globalThis.__transgateCapture(filename,new Uint8Array(pdf.output("arraybuffer")))'), loader: 'ts', resolveDir: dirname(args.path)};
    });
  }}],
});
const module = {exports: {}};
new Function('require', 'module', 'exports', result.outputFiles[0].text)(require, module, module.exports);
const source = module.exports;
await mkdir(out, {recursive: true});
for (const c of cases) {
  active = c.name; pending = [];
  const data = c.progress.analysis ? {...c.progress.analysis} : {};
  delete data.type;
  if (c.progress.summary !== undefined) data.summary = c.progress.summary;
  const summary = data.summary || '';
  const highlights = (data.highlights || []).map(h => ({text: h.text || h, category: h.category || 'highlight', timestamp: h.timestamp}));
  const captured = {summary, highlights, specializedData: data, metadata: {title: `Fixture ${c.name}`, date: new Date(0), analysisType: c.progress.detected?.kind || 'generic', jobId: 7}};
  await writeFile(join(out, `${c.name}.txt`), source.formatFullAnalysisAsText(summary, highlights, data));
  await writeFile(join(out, `${c.name}.md`), source.formatFullAnalysisAsText(summary, highlights, data));
  await writeFile(join(out, `${c.name}.html`), source.formatFullAnalysisAsHtml(summary, highlights, data));
  const docx = await source.exportAsDocx(captured); if (!docx.success) throw new Error(`${c.name} DOCX: ${docx.error}`);
  const pdf = await source.exportAsPdf(captured); if (!pdf.success) throw new Error(`${c.name} PDF: ${pdf.error}`);
  // The save adapter can enqueue its write after resolving the Blob; drain
  // that queue without sleeping, retrying, altering output or swallowing errors.
  for (let i = 0; i < pending.length; i++) await pending[i];
}
for (const [name, customBillTo] of [['invoice', ''], ['admin-invoice', 'Temporary Company\nمرحبا بالعالم\nTax 456']]) {
  const input = {payment: {id:42, created:'2026-10-25T12:00:00Z', amount:120, currency:'TRY', payment_gateway:'iyzico', metadata:{package:{name:'Professional', minutes:600}}}, user:{email:'owner@example.test'}, userDetails:{full_name:'Owner'}, billingInfo:{address:'Stored address'}, customBillTo};
  await writeFile(join(out, `${name}.pdf`), await source.invoice(input));
}
const files = [join(exporter, 'docx-exporter.ts'), join(exporter, 'pdf-exporter.ts'), utils, billing, casesPath];
await writeFile(join(out, 'capture-provenance.json'), JSON.stringify({
  source: frontend, timezone: 'UTC', click_time:'1970-01-01T00:00:00.000Z', synthetic_only:true,
  mechanics:['file-saver intercepted to save Blob','jsPDF.save intercepted to save output(arraybuffer)','Arabic CDN fetch replaced by unchanged locally bundled font','Invoice JSX declarations selected by TypeScript AST; source logo supplied from the same source tree'],
  formatting:'original source functions; no formatter replacement',
  library_entries: await Promise.all(Object.keys(globalThis.__transgatePackages).map(async name => {
    const file = require.resolve(name);
    return {name, file, sha256:createHash('sha256').update(await readFile(file)).digest('hex')};
  })),
  source_package_sha256:createHash('sha256').update(await readFile(join(frontend,'package.json'))).digest('hex'),
  arabic_font_sha256:createHash('sha256').update(font).digest('hex'),
  files: await Promise.all(files.map(async file => ({file, sha256:createHash('sha256').update(await readFile(file)).digest('hex')}))),
}, null, 2)+'\n');
globalThis.fetch = originalFetch;
console.log(`Source outputs captured in ${out}; independent readers and Arabic visual comparison still must run.`);
