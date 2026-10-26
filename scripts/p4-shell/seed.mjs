#!/usr/bin/env node
// Enroll/seed one synthetic dev-auth owner against an already running disposable
// host. Uses the existing published identity mapping and the host owner's SQL.
import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required (owned local fixtures only)`);
  return value;
}
const api = required("TASKR_P4_API").replace(/\/$/, "");
const email = required("TASKR_P4_EMAIL");
const database = required("TASKR_P4_FIXTURE_DATABASE_URL");
const work = process.env.TASKR_P4_WORK_ID ?? randomUUID();
const digest = createHash("sha1")
  .update(Buffer.from("3f0e5a9162c44d7b9e18a2c671b408d5", "hex"))
  .update(`dev-${email}`).digest().subarray(0, 16);
digest[6] = (digest[6] & 15) | 0x50;
digest[8] = (digest[8] & 63) | 0x80;
const hex = digest.toString("hex");
const account = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
const enrolled = await fetch(`${api}/audio-work/enroll`, { method: "POST", headers: { Authorization: `Bearer dev:${email}` } });
if (!enrolled.ok) throw new Error(`enrollment failed ${enrolled.status}: ${await enrolled.text()}`);
const fixture = fileURLToPath(new URL("../p4-fixtures/taskr-completed.sql", import.meta.url));
execFileSync("psql", [database, "-v", "ON_ERROR_STOP=1", "-v", `account_id=${account}`, "-v", `work_id=${work}`, "-f", fixture], { stdio: ["ignore", "ignore", "inherit"] });
const detail = await fetch(`${api}/audio-work/${work}`, { headers: { Authorization: `Bearer dev:${email}` } });
if (!detail.ok) throw new Error(`seeded work is not owner-readable: ${detail.status}`);
const body = await detail.json();
if (body.id !== work || body.status !== "completed" || !body.transcript) throw new Error("seed did not create a completed owned transcript");
console.log(JSON.stringify({ email, account_id: account, work_id: work }));
