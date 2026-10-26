// Independent headless fixture evidence; execute only against a lead-released snapshot.
import { readFile, mkdir, writeFile, appendFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const root=process.argv[2];
if(!root)throw new Error('immutable snapshot root required');
const {chromium,devices}=await import(pathToFileURL(path.join(root,'src/transgate/web/node_modules/playwright/index.mjs')).href);
const out=path.join(root,'logs/next-p5-c/c1');
await mkdir(out,{recursive:true});
if(existsSync(path.join(out,'results.jsonl')))throw new Error('Refusing to overwrite or append an earlier matrix');
const staticRoot=path.join(root,'src/transgate/web/storybook-static');
const index=JSON.parse(await readFile(path.join(staticRoot,'index.json'),'utf8'));
const stories=Object.values(index.entries).filter(x=>x.type==='story');
const axe=await readFile(path.join(root,'src/transgate/web/node_modules/axe-core/axe.min.js'),'utf8');
const browser=await chromium.launch({headless:true,executablePath:'/home/mu/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome'});
const results=[];
function requestEvidence(request,status,error){
 const url=new URL(request.url());
 const file=path.resolve(staticRoot,'.'+decodeURIComponent(url.pathname));
 return {url:request.url(),status,error,resourceType:request.resourceType(),chunkPresent:url.hostname==='127.0.0.1'&&file.startsWith(staticRoot+path.sep)?existsSync(file):null};
}
try {
 for(const palette of ['paper-moss','paper-indigo','paper-ochre']) for(const mode of ['light','dark','system-light','system-dark']) {
  const dir=path.join(out,`${palette}-${mode}`);await mkdir(dir,{recursive:true});
  for(const story of stories){
   const phone=/MobileViews/.test(story.title)||/phone/i.test(story.name);
   const context=await browser.newContext(phone?{...devices['Pixel 7']}:{viewport:{width:1280,height:800}});
   const page=await context.newPage();const errors=[],external=[],requestFailures=[];
   page.on('pageerror',e=>errors.push(String(e)));
   page.on('requestfailed',r=>requestFailures.push(requestEvidence(r,null,r.failure()?.errorText??'unknown transport failure')));
   page.on('response',r=>{if(r.status()>=400)requestFailures.push(requestEvidence(r.request(),r.status(),null));});
   await context.route('**/*',route=>{const u=new URL(route.request().url());if(u.hostname==='127.0.0.1'||['data:','blob:'].includes(u.protocol))return route.continue();external.push(u.origin);return route.abort();});
   const row={id:story.id,title:story.title,project:phone?'Pixel 7':'Desktop Chrome',locale:'en',palette,mode,errors,external,requestFailures};
   try{
    await page.emulateMedia({colorScheme:mode.endsWith('dark')?'dark':'light'});
    const theme=mode.startsWith('system')?'system':mode;
    // addon-a11y 10.6.2 afterEach checks this global before starting its own audit.
    await page.goto(`http://127.0.0.1:49431/iframe.html?id=${story.id}&viewMode=story&globals=theme:${theme};palette:${palette};locale:en;a11y.manual:!true`);
    await page.waitForFunction(()=>document.querySelector('#storybook-root')?.children.length>0||document.querySelector('.sb-errordisplay')?.checkVisibility(),{timeout:10000});
    await page.evaluate(()=>document.fonts.ready);
    row.viewport=await page.evaluate(()=>({width:innerWidth,height:innerHeight,dpr:devicePixelRatio}));
    row.renderError=await page.locator('.sb-errordisplay').isVisible()?await page.locator('.sb-errordisplay').innerText():null;
    await page.screenshot({path:path.join(dir,story.id+'.png'),fullPage:true});
    if(row.renderError)throw new Error('Story did not render; accessibility audit not attempted');
    // Public StoryStore.getStoryContext(PreparedStory) reads effective story globals.
    row.auditManual=await page.evaluate(()=>{const r=globalThis.__STORYBOOK_PREVIEW__?.currentRender;return r?.story?r.store.getStoryContext(r.story).globals.a11y?.manual:null;});
    if(row.auditManual!==true)throw new Error('Harness precondition: effective a11y.manual must be true before independent axe.run');
    await page.addScriptTag({content:axe});
    row.axe=await page.evaluate(async()=>{const r=await axe.run(document.querySelector('#storybook-root'),{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21aa']}});return r.violations.map(v=>({id:v.id,impact:v.impact,description:v.description,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}));});
    row.focus=[];
    for(let i=0;i<6;i++){await page.keyboard.press('Tab');row.focus.push(await page.evaluate(()=>{const e=document.activeElement;const s=getComputedStyle(e);return {tag:e.tagName,text:e.getAttribute('aria-label')||Array.from(e.textContent?.trim()??'').slice(0,100).join(''),outline:s.outline,boxShadow:s.boxShadow,insideDialog:!!e.closest('dialog')};}));}
    row.theme=await page.evaluate(()=>({theme:document.documentElement.dataset.theme??null,palette:document.documentElement.dataset.palette??null,overflow:document.documentElement.scrollWidth>innerWidth}));
   }catch(e){row.failure=String(e);}
   results.push(row);await appendFile(path.join(out,'results.jsonl'),JSON.stringify(row)+'\n');await context.close();
  }
  console.log('completed',palette,mode,results.length);
 }
}finally{await browser.close();await writeFile(path.join(out,'report.json'),JSON.stringify({stories:stories.length,cases:results.length,unprepared:results.filter(r=>r.renderError).length,audited:results.filter(r=>'axe' in r).length,axeFailures:results.filter(r=>r.axe?.length).length,results},null,2));}
