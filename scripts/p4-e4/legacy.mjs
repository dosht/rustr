// Execute the inspected source fetch path; never import source app configuration.
import { readFileSync, realpathSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import path from 'node:path';
import vm from 'node:vm';

const MODULES = new Set([
  'ai-service-manager', 'deepseek-provider', 'openai-provider', 'content-processor',
  'prompt-templates', 'schemas', 'schema-to-prompt', 'locale-map', 'chat-config',
  'chat-validator', 'types', 'validation',
]);
const UNUSED = new Set(['cleanup-service', 'consent-service', 'provider-factory']);
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const forbidden = name => { throw new Error(`E4 blocked unused source dependency: ${name}`); };

export function createLegacy(sourceRoot, fetchFixture) {
  const root = realpathSync(sourceRoot);
  const aiRoot = realpathSync(path.join(root, 'src/lib/ai'));
  const sourceRequire = createRequire(path.join(root, 'package.json'));
  // These are existing local dependencies, not downloaded or installed by E4.
  const ts = sourceRequire('typescript');
  const zod = sourceRequire('zod');
  const provenance = [];
  const warnings = [];
  const cache = new Map();
  const logger = Object.fromEntries(['debug', 'info', 'warn', 'error'].map(level =>
    [level, (...args) => { if (level === 'warn' || level === 'error') warnings.push({ level, args }); }]));
  const context = vm.createContext({
    TextDecoder, TextEncoder, fetch: fetchFixture, console: logger,
    process: Object.freeze({ env: Object.freeze({ NODE_ENV: 'test', OPENAI_MODEL: 'gpt-3.5-turbo' }) }),
  }, { codeGeneration: { strings: false, wasm: false } });

  function load(name) {
    if (!MODULES.has(name)) forbidden(name);
    if (cache.has(name)) return cache.get(name).exports;
    const filename = realpathSync(path.join(aiRoot, `${name}.ts`));
    if (path.dirname(filename) !== aiRoot) forbidden(filename);
    const bytes = readFileSync(filename);
    provenance.push({ path: `src/lib/ai/${name}.ts`, sha256: digest(bytes) });
    const compiled = ts.transpileModule(bytes.toString('utf8'), {
      compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, esModuleInterop: true },
      fileName: filename,
    }).outputText;
    const module = { exports: {} };
    cache.set(name, module);
    const requireIsolated = specifier => {
      if (specifier === 'zod') return zod;
      if (specifier === '@/utils/logger') return { logger };
      if (specifier === '@/utils/user') return { isFreemiumUser: () => forbidden('billing') };
      // Axios only constructs the UNUSED availability client. Completions use real fetch.
      if (specifier === 'axios') return { create: () => ({ get: () => forbidden('availability HTTP') }) };
      if (specifier.startsWith('./')) {
        const dependency = specifier.slice(2);
        if (UNUSED.has(dependency)) {
          return new Proxy({}, { get: (_, symbol) => () => forbidden(`${dependency}.${String(symbol)}`) });
        }
        return load(dependency);
      }
      return forbidden(specifier);
    };
    new vm.Script(`(function(require,module,exports){\n${compiled}\n})`, { filename })
      .runInContext(context)(requireIsolated, module, module.exports);
    return module.exports;
  }

  const { AIServiceManager } = load('ai-service-manager');
  const { ContentProcessor } = load('content-processor');
  const { UNIFIED_ANALYSIS_PROMPT } = load('prompt-templates');
  const { DeepseekProvider } = load('deepseek-provider');
  const { OpenAIProvider } = load('openai-provider');
  const { validateArrayForRender } = load('validation');
  const { ActionItemSchema } = load('schemas');

  // Execute the exact source route's stream-processing statements, not a rewritten
  // old parser. Authentication/DB/provider dispatch remain outside this pure slice.
  const routePath = realpathSync(path.join(root, 'src/pages/api/v1/ai/analyze.ts'));
  const routeBytes = readFileSync(routePath);
  provenance.push({ path: 'src/pages/api/v1/ai/analyze.ts', sha256: digest(routeBytes) });
  const route = ts.createSourceFile(routePath, routeBytes.toString('utf8'), ts.ScriptTarget.ES2022, true);
  const functions = new Map(route.statements.filter(ts.isFunctionDeclaration).map(node => [node.name?.text, node]));
  const helperSource = ['parseDocument', 'writeSSEEvent'].map(name => {
    const declaration = functions.get(name);
    if (!declaration) throw new Error(`E4 source route helper changed: ${name}`);
    return declaration.getText(route);
  }).join('\n');
  const loops = [];
  const findLoop = node => {
    if (ts.isForOfStatement(node) && node.awaitModifier && node.expression.getText(route) === 'stream') loops.push(node);
    ts.forEachChild(node, findLoop);
  };
  const handler = functions.get('handler');
  if (!handler) throw new Error('E4 source analysis handler changed');
  findLoop(handler);
  if (loops.length !== 1 || !ts.isBlock(loops[0].parent)) throw new Error('E4 source analysis stream block changed');
  const statements = [...loops[0].parent.statements];
  const start = statements.findIndex(node => ts.isVariableStatement(node)
    && node.declarationList.declarations.some(declaration => declaration.name.getText(route) === 'buffer'));
  const end = statements.findIndex(node => ts.isExpressionStatement(node) && node.expression.getText(route) === 'res.end()');
  if (start < 0 || end <= start || !statements.slice(start, end).includes(loops[0])) {
    throw new Error('E4 cannot identify the exact source analysis processing slice');
  }
  const slice = statements.slice(start, end + 1).map(node => node.getText(route)).join('\n');
  const analysisModule = { exports: {} };
  const analysisCode = ts.transpileModule(`${helperSource}\nexport async function capture(stream, transcriptLines, res, logger) {
    const startTime = Date.now();
    ${slice}
    return { rawContent, tokensUsed };
  }`, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
  new vm.Script(`(function(module,exports){${analysisCode}\n})`, { filename: `${routePath}:E4-extracted-stream-slice` })
    .runInContext(context)(analysisModule, analysisModule.exports);

  return {
    analysisTemplate: () => ({ system: UNIFIED_ANALYSIS_PROMPT.system, user: UNIFIED_ANALYSIS_PROMPT.user }),
    renderActionItems: data => validateArrayForRender(ActionItemSchema, data, 'E4-source-ActionItems'),
    async sourceAnalysis(chunks, lines) {
      const frames = [];
      let ended = false;
      const res = { write: frame => frames.push(frame), flush() {}, end() { ended = true; } };
      const stream = (async function* () { yield* chunks; })();
      const metadata = await analysisModule.exports.capture(stream, lines, res, logger);
      const events = frames.map(frame => {
        const match = /^event: ([^\n]+)\ndata: ([^\n]+)\n\n$/.exec(frame);
        if (!match) throw new Error('E4 source route emitted an unexpected frame');
        return { event: match[1], data: JSON.parse(match[2]) };
      });
      return { events, frames, ended, ...metadata };
    },
    provenance: () => ({
      root, files: provenance.slice().sort((a, b) => a.path.localeCompare(b.path)),
      typescript: ts.version, zod: sourceRequire('zod/package.json').version,
      isolation: 'No constructor for AIServiceManager; injected source provider/content processor, no consent service or Supabase. Actual manager generation methods, prompt generator, schemas, sanitation and fetch provider execute. Axios availability constructor stubbed; availability/DB/billing/factory paths forbidden. No SDK completion path.',
      environment: { NODE_ENV: 'test', OPENAI_MODEL: 'gpt-3.5-turbo' },
    }),
    async run(testCase, baseUrl, omitAnalysisMode = false) {
      warnings.length = 0;
      const Provider = testCase.provider === 'openai' ? OpenAIProvider : DeepseekProvider;
      const provider = new Provider({ apiKey: 'e4-synthetic-not-a-secret', baseUrl });
      const chunks = [];
      const original = provider.chatCompletionStream.bind(provider);
      provider.chatCompletionStream = async function* (messages, options) {
        // Only the separately named correction changes the source response mode.
        for await (const chunk of original(messages, omitAnalysisMode ? undefined : options)) {
          chunks.push(chunk);
          yield chunk;
        }
      };
      const manager = Object.create(AIServiceManager.prototype);
      manager.provider = provider;
      manager.contentProcessor = new ContentProcessor();
      manager.consentService = null;
      manager.supabase = undefined;
      let final = null;
      let error = null;
      try {
        if (testCase.kind === 'analysis') {
          for await (const _chunk of manager.generateWithTemplateStream(
            UNIFIED_ANALYSIS_PROMPT, testCase.lines, { language: testCase.language },
          )) { /* Provider wrapper records exactly the delivered chunks. */ }
        } else if (testCase.kind === 'summary') {
          const result = await manager.generateSummary(1, testCase.lines,
            testCase.detailed ? 'detailed' : 'general', { language: testCase.language });
          if (result.status !== 'success') error = { message: result.error };
          else final = result.data;
        } else if (testCase.kind === 'chat') {
          for await (const event of manager.processChatMessageStream(
            testCase.question, testCase.lines, testCase.history ?? [], undefined, undefined,
            { language: testCase.language },
          )) {
            if (event.type === 'done') final = event.content;
            if (event.type === 'error') error = { message: event.error };
          }
        } else throw new Error(`Unknown E4 operation ${testCase.kind}`);
      } catch (failure) {
        error = { code: failure.code ?? null, message: failure.message ?? String(failure) };
      }
      return { chunks, content: chunks.join(''), final, error, warnings: [...warnings] };
    },
  };
}
