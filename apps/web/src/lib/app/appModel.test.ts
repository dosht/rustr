import { describe, expect, it, vi } from "vitest";
import type { AppCore, AppView } from "../core";
import { appScreens } from "../core/fixtures";
import { createAppModel } from "./appModel.svelte";

function fakeCore(views: AppView[]): AppCore & { dispatch: ReturnType<typeof vi.fn> } {
  let current = views[0];
  const dispatch = vi.fn(async () => {
    current = views.shift() ?? current;
    return current;
  });
  return { dispatch, current: () => current };
}

describe("createAppModel", () => {
  it("starts the app and forwards every intent to the core", async () => {
    const core = fakeCore([appScreens["sign-in"], appScreens["owner-tasks"]]);
    const model = await createAppModel(core);
    expect(core.dispatch).toHaveBeenCalledWith({ type: "Started" });
    expect(model.view.state).toBe("signedOut");

    await model.send({ type: "SignInSubmitted", email: "ada@example.com", password: "x" });
    expect(model.view.state).toBe("signedIn");
  });

  it("shows the latest state even when an older dispatch finishes last", async () => {
    let finishSlow: () => void = () => {};
    let current: AppView = appScreens["owner-tasks"];
    const core: AppCore = {
      dispatch: (intent) =>
        intent.type === "Started"
          ? Promise.resolve(current)
          : new Promise((resolve) => {
              finishSlow = () => resolve(appScreens["loading"]);
            }),
      current: () => current,
    };
    const model = await createAppModel(core);
    const slow = model.send({ type: "WorkspaceSelected", id: "ws-2" });
    current = appScreens["owner-members"];
    finishSlow();
    await slow;
    expect(model.view).toEqual(appScreens["owner-members"]);
  });

  it("shows the pending view while requests run", async () => {
    let finish: () => void = () => {};
    let current: AppView = appScreens["owner-tasks"];
    const core: AppCore = {
      dispatch: (intent, onView) => {
        if (intent.type === "Started") return Promise.resolve(current);
        current = appScreens["loading"];
        onView?.(current);
        // ES2022 lib has no Promise.withResolvers.
        return new Promise((resolve) => {
          finish = () => resolve((current = appScreens["owner-members"]));
        });
      },
      current: () => current,
    };
    const model = await createAppModel(core);
    const pending = model.send({ type: "WorkspaceSelected", id: "ws-2" });
    expect(model.view).toEqual(appScreens["loading"]);
    finish();
    await pending;
    expect(model.view).toEqual(appScreens["owner-members"]);
  });
});
