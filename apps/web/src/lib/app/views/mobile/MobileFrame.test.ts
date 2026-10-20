import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { signedIn } from "../../../core/fixtures";
import MobileFrame from "./MobileFrame.svelte";

describe("MobileFrame", () => {
  it("has a title bar and bottom tabs instead of a sidebar", () => {
    render(MobileFrame, { view: signedIn("owner-tasks"), onIntent: vi.fn() });
    expect(screen.getByText("ada's workspace · Tasks")).toBeInTheDocument();
    const tabs = screen.getByRole("navigation", { name: "Sections" });
    expect(within(tabs).getByRole("button", { name: "Tasks" })).toHaveAttribute("aria-current", "page");
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("switches workspace from the drawer and closes it", async () => {
    const onIntent = vi.fn();
    render(MobileFrame, { view: signedIn("owner-tasks"), onIntent });
    await userEvent.click(screen.getByRole("button", { name: "Open workspaces" }));
    const drawer = screen.getByRole("complementary", { name: "Workspaces and account" });
    await userEvent.click(within(drawer).getByRole("button", { name: /Team/ }));
    expect(onIntent).toHaveBeenCalledWith({ type: "WorkspaceSelected", id: "ws-2" });
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("closes the drawer from its close button or Escape", async () => {
    render(MobileFrame, { view: signedIn("owner-tasks"), onIntent: vi.fn() });
    await userEvent.click(screen.getByRole("button", { name: "Open workspaces" }));
    await userEvent.click(screen.getByRole("button", { name: "Close workspaces" }));
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open workspaces" }));
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("uses the mobile screen views", () => {
    render(MobileFrame, { view: signedIn("viewer-tasks"), onIntent: vi.fn() });
    expect(screen.getByRole("button", { name: "Write the architecture doc, open" })).toBeDisabled();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("sends screen intents with the workspace they were shown for", async () => {
    const onIntent = vi.fn();
    const { unmount } = render(MobileFrame, { view: signedIn("owner-tasks"), onIntent });
    await userEvent.click(screen.getByRole("tab", { name: "All (4)" }));
    expect(onIntent).toHaveBeenCalledWith({
      type: "Tasks",
      workspace: "ws-1",
      intent: { type: "ScopeChanged", scope: "all" },
    });
    unmount();
    render(MobileFrame, { view: signedIn("owner-members"), onIntent });
    await userEvent.selectOptions(screen.getByLabelText("Role of bob@example.com"), "viewer");
    expect(onIntent).toHaveBeenCalledWith({
      type: "Members",
      workspace: "ws-1",
      intent: { type: "RoleChanged", email: "bob@example.com", role: "viewer" },
    });
  });
});
