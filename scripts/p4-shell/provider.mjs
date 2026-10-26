#!/usr/bin/env node
// Synthetic loopback completion server for real taskr host/browser/UniFFI proof.
// It deliberately holds completion until the external harness releases it.
import http from "node:http";
import { randomUUID } from "node:crypto";

const port = Number(process.env.TASKR_P4_PROVIDER_PORT ?? 18891);
const runs = new Map();
function json(response, status, value) {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(value));
}
function delta(run, content, finishReason = null) {
  if (run.response.destroyed) return;
  run.response.write(`data: ${JSON.stringify({ id: run.id, choices: [{ index: 0, delta: { content }, finish_reason: finishReason }] })}\n\n`);
}
const server = http.createServer(async (request, response) => {
  const url = new URL(request.url, "http://127.0.0.1");
  if (request.method === "GET" && url.pathname === "/runs") {
    return json(response, 200, { runs: [...runs.values()].map(({ id, finished, model }) => ({ id, finished, model })) });
  }
  const release = /^\/runs\/([^/]+)\/finish$/.exec(url.pathname);
  if (request.method === "POST" && release) {
    const run = runs.get(release[1]);
    if (!run) return json(response, 404, { error: "unknown run" });
    if (!run.finished) {
      run.finished = true;
      delta(run, "world  ");
      delta(run, "", "stop");
      if (!run.response.destroyed) run.response.end("data: [DONE]\n\n");
    }
    return json(response, 200, { id: run.id, finished: true });
  }
  if (request.method !== "POST" || !url.pathname.endsWith("/chat/completions")) return json(response, 404, { error: "unknown route" });
  if (request.headers.authorization !== "Bearer p4-synthetic-key") return json(response, 401, { error: "synthetic key required" });
  let bytes = 0;
  const chunks = [];
  for await (const chunk of request) {
    bytes += chunk.length;
    if (bytes > 16 * 1024 * 1024) { request.destroy(); return; }
    chunks.push(chunk);
  }
  let body;
  try { body = JSON.parse(Buffer.concat(chunks).toString("utf8")); }
  catch { return json(response, 400, { error: "invalid JSON" }); }
  if (body.stream !== true) return json(response, 400, { error: "stream required" });
  response.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
  const run = { id: `p4-${randomUUID()}`, model: body.model, finished: false, response };
  runs.set(run.id, run);
  // The host checkpoint trims this to Hello. Appending raw world after resume
  // would incorrectly yield Helloworld instead of the authoritative Hello world.
  delta(run, "Hello ");
});
server.listen(port, "127.0.0.1", () => console.log(`P4_PROVIDER_READY http://127.0.0.1:${port}`));
function stop() {
  for (const run of runs.values()) run.response.destroy();
  server.close();
  server.closeAllConnections();
}
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
