import { render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import { appScreens } from "../core/fixtures";
import AppContainer from "./AppContainer.svelte";

const model = (name: string) => ({ view: appScreens[name], send: vi.fn() });

describe("AppContainer", () => {
  it("picks the desktop frame on wide screens and the mobile frame on phones", () => {
    const { unmount } = render(AppContainer, { model: model("owner-tasks"), mobile: false, devSignIn: false });
    expect(screen.getByRole("navigation", { name: "Workspaces" })).toBeInTheDocument();
    unmount();
    render(AppContainer, { model: model("owner-tasks"), mobile: true, devSignIn: false });
    expect(screen.getByRole("button", { name: "Open workspaces" })).toBeInTheDocument();
  });

  it("shows sign-in whenever the core says signed out", () => {
    render(AppContainer, { model: model("sign-in"), mobile: true, devSignIn: false });
    expect(screen.getByRole("button", { name: "Sign in" })).toBeInTheDocument();
  });
});
