// Runs the real wasm core under Node: node --localstorage-file=<path> scripts/smoke.mjs
import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import { initSync, TaskListPresenter, previewScreens } from "../src/lib/core/pkg/taskr_wasm.js";

initSync({ module: readFileSync(new URL("../src/lib/core/pkg/taskr_wasm_bg.wasm", import.meta.url)) });
localStorage.clear();

const presenter = new TaskListPresenter();
presenter.dispatch({ type: "Opened" });
let view = presenter.dispatch({ type: "AddSubmitted", title: "Buy milk" });
assert.deepEqual(view.rows.map((r) => r.title), ["Buy milk"]);
assert.equal(view.notice, 'Added "Buy milk"');

view = presenter.dispatch({ type: "AddSubmitted", title: "BUY MILK" });
assert.equal(view.error, "There is already an open task with that title.");

view = presenter.dispatch({ type: "ToggleRequested", id: 1 });
assert.equal(view.tabs[1].label, "Done (1)");
assert.equal(view.emptyMessage, "All done. Nice.");

const reopened = new TaskListPresenter().dispatch({ type: "Opened" });
assert.equal(reopened.tabs[2].label, "All (1)", "data survives a new presenter via localStorage");

assert.deepEqual(previewScreens().map((s) => s.name), ["empty", "loading", "sample", "error"]);
console.log("wasm smoke test passed");
