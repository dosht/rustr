// Builds the book: Markdown chapters -> checked snippets + rendered diagrams
// -> EPUB (pandoc) and PDF (pandoc HTML printed by headless Chromium).
//
//   node tools/build.mjs           build everything into build/
//   node tools/build.mjs --check   only verify snippets against the source
//
// Environment: PANDOC (default "pandoc"), CHROME_PATH (default: newest
// Playwright Chromium in ~/.cache/ms-playwright), BOOK_LAX=1 to warn instead
// of failing on stale snippets.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const BOOK = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const REPO = path.resolve(BOOK, "..");
const OUT = path.join(BOOK, "build");
const SRC = path.join(OUT, "src");
const DIAGRAMS = path.join(OUT, "diagrams");
const PANDOC = process.env.PANDOC || "pandoc";
const SLUG = "core-and-shells";
const checkOnly = process.argv.includes("--check");

const FENCE = /^```(\S*)\s*(\{[^}]*\})?\s*$/;

function chapters() {
  const dir = path.join(BOOK, "chapters");
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith(".md"))
    .sort()
    .map((f) => ({ name: f, text: fs.readFileSync(path.join(dir, f), "utf8") }));
}

function attrs(braced) {
  const out = { classes: [], kv: {} };
  if (!braced) return out;
  for (const m of braced.slice(1, -1).matchAll(/\.([\w-]+)|([\w-]+)="([^"]*)"/g)) {
    if (m[1]) out.classes.push(m[1]);
    else out.kv[m[2]] = m[3];
  }
  return out;
}

// A snippet is honest if each run of lines between "..." markers appears,
// in order and contiguously, in the named file (indentation ignored).
function snippetProblem(code, file) {
  const full = path.join(REPO, file);
  if (!fs.existsSync(full)) return `missing file ${file}`;
  const source = fs.readFileSync(full, "utf8").split("\n").map((l) => l.trim());
  const isGap = (l) => /^(\/\/|#|<!--)?\s*\.\.\.\s*(-->)?$/.test(l.trim());
  const segments = [[]];
  for (const line of code.split("\n")) {
    if (isGap(line)) segments.push([]);
    else segments.at(-1).push(line.trim());
  }
  let from = 0;
  for (const seg of segments.filter((s) => s.length)) {
    let found = -1;
    for (let i = from; i <= source.length - seg.length && found < 0; i++) {
      if (seg.every((l, j) => source[i + j] === l)) found = i;
    }
    if (found < 0) return `"${seg[0]}" ... not found in ${file}`;
    from = found + seg.length;
  }
  return null;
}

function preprocess(chapter, diagrams, problems) {
  const lines = chapter.text.split("\n");
  const out = [];
  for (let i = 0; i < lines.length; i++) {
    const open = lines[i].match(FENCE);
    if (!open || (!open[1] && !open[2])) {
      out.push(lines[i]);
      continue;
    }
    const body = [];
    let j = i + 1;
    while (j < lines.length && !/^```\s*$/.test(lines[j])) body.push(lines[j++]);
    const code = body.join("\n");
    const { classes, kv } = attrs(open[2]);
    const lang = open[1] || classes[0] || "";

    if (lang === "mermaid") {
      const caption = (code.match(/^%%\s*caption:\s*(.+)$/m) || [])[1] || "";
      const hash = createHash("sha1").update(code).digest("hex").slice(0, 12);
      diagrams.set(hash, code);
      out.push(`![${caption}](diagrams/${hash}.png)`);
    } else {
      if (kv.file) {
        const problem = snippetProblem(code, kv.file);
        if (problem) problems.push(`${chapter.name}: ${problem}`);
      }
      out.push("```" + lang, ...body, "```");
      if (kv.file) out.push("", `::: source`, `\`${kv.file}\``, `:::`);
    }
    i = j;
  }
  return out.join("\n");
}

function chromePath() {
  if (process.env.CHROME_PATH) return process.env.CHROME_PATH;
  const cache = path.join(os.homedir(), ".cache", "ms-playwright");
  const builds = fs.existsSync(cache)
    ? fs.readdirSync(cache).filter((d) => /^chromium-\d+$/.test(d)).sort().reverse()
    : [];
  for (const b of builds) {
    for (const sub of ["chrome-linux64/chrome", "chrome-linux/chrome", "chrome-mac/Chromium.app/Contents/MacOS/Chromium"]) {
      const p = path.join(cache, b, sub);
      if (fs.existsSync(p)) return p;
    }
  }
  throw new Error("No Chromium found. Set CHROME_PATH.");
}

async function withBrowser(fn) {
  const { chromium } = await import("playwright-core");
  const browser = await chromium.launch({ executablePath: chromePath() });
  try {
    return await fn(browser);
  } finally {
    await browser.close();
  }
}

async function renderDiagrams(browser, diagrams) {
  fs.mkdirSync(DIAGRAMS, { recursive: true });
  for (const f of fs.readdirSync(DIAGRAMS)) {
    if (!diagrams.has(f.replace(/\.png$/, ""))) fs.rmSync(path.join(DIAGRAMS, f));
  }
  const todo = [...diagrams].filter(([hash]) => !fs.existsSync(path.join(DIAGRAMS, `${hash}.png`)));
  if (!todo.length) return;
  const page = await browser.newPage({ deviceScaleFactor: 2, viewport: { width: 900, height: 600 } });
  await page.setContent(`<html><body style="margin:0;background:#fff"><div id="d" style="display:inline-block;padding:16px"></div></body></html>`);
  await page.addScriptTag({ path: path.join(BOOK, "node_modules/mermaid/dist/mermaid.min.js") });
  await page.evaluate(() =>
    window.mermaid.initialize({
      startOnLoad: false,
      theme: "neutral",
      fontFamily: "Liberation Sans, Arial, sans-serif",
      flowchart: { htmlLabels: true, curve: "basis" },
    }),
  );
  for (const [hash, code] of todo) {
    await page.evaluate(async ({ hash, code }) => {
      const { svg } = await window.mermaid.render(`m${hash}`, code);
      document.getElementById("d").innerHTML = svg;
    }, { hash, code });
    await page.locator("#d").screenshot({ path: path.join(DIAGRAMS, `${hash}.png`) });
    console.log(`  diagram ${hash}`);
  }
  await page.close();
}

async function renderCover(browser, version) {
  const meta = fs.readFileSync(path.join(BOOK, "metadata.yaml"), "utf8");
  const field = (k) => (meta.match(new RegExp(`^${k}:\\s*"?(.*?)"?$`, "m")) || [])[1] || "";
  const page = await browser.newPage({ viewport: { width: 800, height: 1280 }, deviceScaleFactor: 2 });
  await page.setContent(`<html><body style="margin:0">
    <div style="width:800px;height:1280px;box-sizing:border-box;padding:96px 72px;background:#1a1b26;color:#e6e6e6;
      font-family:'Liberation Sans',Arial,sans-serif;display:flex;flex-direction:column;justify-content:space-between">
      <div>
        <div style="font-size:28px;letter-spacing:6px;color:#8ec07c">TASKR</div>
        <div style="font-size:96px;font-weight:700;line-height:1.05;margin-top:48px">${field("title")}</div>
        <div style="font-size:34px;line-height:1.35;margin-top:40px;color:#c8cbd6">${field("subtitle")}</div>
      </div>
      <div style="font-family:'JetBrainsMono NF','Noto Sans Mono',monospace;font-size:22px;line-height:1.8;color:#8a8fa8">
        domain &rarr; use cases &rarr; presentation<br/>cli &middot; tui &middot; web &middot; android &middot; ios
        <div style="margin-top:40px;font-family:'Liberation Sans',Arial,sans-serif;font-size:28px;color:#e6e6e6">${field("author")}</div>
        <div style="font-size:20px">${version}</div>
      </div>
    </div></body></html>`);
  await page.screenshot({ path: path.join(OUT, "cover.png") });
  await page.close();
}

function version() {
  const date = new Date().toISOString().slice(0, 10);
  try {
    const rev = execFileSync("git", ["describe", "--always", "--dirty"], { cwd: REPO }).toString().trim();
    return `Edition ${date} (${rev})`;
  } catch {
    return `Edition ${date}`;
  }
}

function subtitle() {
  const meta = fs.readFileSync(path.join(BOOK, "metadata.yaml"), "utf8");
  return (meta.match(/^subtitle:\s*"?(.*?)"?$/m) || [])[1] || "";
}

function pandoc(args) {
  execFileSync(PANDOC, args, { cwd: OUT, stdio: "inherit" });
}

async function main() {
  const diagrams = new Map();
  const problems = [];
  const sources = chapters().map((c) => ({ name: c.name, text: preprocess(c, diagrams, problems) }));

  if (problems.length) {
    console.error(`Stale snippets (the code moved on, update the book):\n  ${problems.join("\n  ")}`);
    if (!process.env.BOOK_LAX) process.exit(1);
  }
  console.log(`snippets ok, ${sources.length} chapters, ${diagrams.size} diagrams`);
  if (checkOnly) return;

  fs.rmSync(SRC, { recursive: true, force: true });
  fs.mkdirSync(SRC, { recursive: true });
  for (const s of sources) fs.writeFileSync(path.join(SRC, s.name), s.text);
  const files = sources.map((s) => path.join("src", s.name));
  const edition = version();

  await withBrowser(async (browser) => {
    await renderDiagrams(browser, diagrams);
    await renderCover(browser, edition);

    const common = [
      `--metadata-file=${path.join(BOOK, "metadata.yaml")}`,
      ...files,
      "--from=markdown",
      "--toc",
      "--toc-depth=2",
      "--number-sections",
      "--syntax-highlighting=pygments",
      "--resource-path=.",
    ];
    const isoDate = new Date().toISOString().slice(0, 10);

    pandoc([...common, "--css", path.join(BOOK, "styles/epub.css"), "--epub-cover-image=cover.png",
      "--split-level=1", "-M", `date=${isoDate}`, "-M", `subtitle=${subtitle()} (${edition})`, "-o", `${SLUG}.epub`]);
    console.log(`  ${SLUG}.epub`);

    pandoc([...common, "--standalone", "--embed-resources", "--css", path.join(BOOK, "styles/pdf.css"),
      "-M", "document-css=false", "-M", `date=${edition}`, "-o", "book.html"]);
    const page = await browser.newPage();
    await page.goto("file://" + path.join(OUT, "book.html"), { waitUntil: "load" });
    await page.pdf({
      path: path.join(OUT, `${SLUG}.pdf`),
      width: "160mm",
      height: "229mm",
      printBackground: true,
      displayHeaderFooter: true,
      headerTemplate: "<span></span>",
      footerTemplate: `<div style="width:100%;font-size:8px;color:#888;text-align:center;font-family:sans-serif">
        <span class="pageNumber"></span></div>`,
      margin: { top: "16mm", bottom: "16mm", left: "14mm", right: "14mm" },
    });
    console.log(`  ${SLUG}.pdf`);
  });
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
