import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { signedIn } from "../../../core/fixtures";
import DesktopFrame from "./DesktopFrame.svelte";

describe("DesktopFrame", () => {
  it("shows workspaces and sections in a sidebar, selected ones marked", () => {
    render(DesktopFrame, { view: signedIn("owner-tasks"), onIntent: vi.fn() });
    const workspaces = screen.getByRole("navigation", { name: "Workspaces" });
    expect(workspaces).toHaveTextContent("ada's workspace");
    expect(screen.getByRole("button", { name: /ada's workspace/ })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Tasks" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("heading", { name: "ada's workspace" })).toBeInTheDocument();
  });

  it("wraps screen intents and navigation as app intents", async () => {
    const onIntent = vi.fn();
    render(DesktopFrame, { view: signedIn("owner-tasks"), onIntent });
    await userEvent.click(screen.getByRole("button", { name: /Team/ }));
    expect(onIntent).toHaveBeenCalledWith({ type: "WorkspaceSelected", id: "ws-2" });
    await userEvent.click(screen.getByRole("button", { name: "Members" }));
    expect(onIntent).toHaveBeenCalledWith({ type: "ScreenSelected", screen: "members" });
    await userEvent.click(screen.getByRole("tab", { name: "All (4)" }));
    expect(onIntent).toHaveBeenCalledWith({
      type: "Tasks",
      workspace: "ws-1",
      intent: { type: "ScopeChanged", scope: "all" },
    });
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    expect(onIntent).toHaveBeenCalledWith({ type: "SignOutRequested" });
  });

  it("renders the members screen when Rust says it is open, tagged with its workspace", async () => {
    const onIntent = vi.fn();
    render(DesktopFrame, { view: signedIn("owner-members"), onIntent });
    expect(screen.getByRole("heading", { name: "Members" })).toBeInTheDocument();
    await userEvent.selectOptions(screen.getByLabelText("Role of bob@example.com"), "owner");
    expect(onIntent).toHaveBeenCalledWith({
      type: "Members",
      workspace: "ws-1",
      intent: { type: "RoleChanged", email: "bob@example.com", role: "owner" },
    });
  });
});
