import { createRawSnippet } from "svelte";
import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopSidebar from "./desktop/DesktopSidebar.svelte";
import AccountSheet from "./mobile/AccountSheet.svelte";
import PhoneDrawer from "./mobile/PhoneDrawer.svelte";

const balance = { ns: "common", key: "Loading...", args: {} } as never;
const list = createRawSnippet(() => ({ render: () => "<ul><li>podcast.mp3</li></ul>" }));

beforeAll(() => i18n.use("en"));

function sidebar(collapsed: boolean) {
  const handlers = { ontoggle: vi.fn(), onnew: vi.fn(), onall: vi.fn(), onbalance: vi.fn(), onsupport: vi.fn(), onupgrade: vi.fn(), onaccount: vi.fn() };
  render(DesktopSidebar, { collapsed, displayName: "Ada Lovelace", balance, signingOut: false, history: list, ...handlers });
  return handlers;
}

describe("DesktopSidebar", () => {
  it("expanded: history slot and account menu without an API-key entry (D13)", async () => {
    const h = sidebar(false);
    expect(screen.getByText("podcast.mp3")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Ada Lovelace" }));
    expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /API Keys/ })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Sign Out" }));
    expect(h.onaccount).toHaveBeenCalledWith("sign_out");
  });

  it("collapsed: icon actions keep their names and the history list is not rendered", async () => {
    const h = sidebar(true);
    expect(screen.getByRole("button", { name: "Expand Sidebar" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("podcast.mp3")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Upgrade Now" }));
    expect(h.onupgrade).toHaveBeenCalledOnce();
  });
});

describe("phone navigation", () => {
  it("drawer navigation closes the drawer; the account row opens the sheet instead", async () => {
    const h = { onclose: vi.fn(), onnew: vi.fn(), onall: vi.fn(), onbalance: vi.fn(), onaccount: vi.fn() };
    render(PhoneDrawer, { open: true, displayName: "Ada Lovelace", balance, recent: list, ...h });
    const drawer = screen.getByRole("dialog", { name: "MAIN" });
    await userEvent.click(within(drawer).getByRole("button", { name: "All Transcripts" }));
    expect(h.onall).toHaveBeenCalledOnce();
    expect(h.onclose).toHaveBeenCalledOnce();
    await userEvent.click(within(drawer).getByRole("button", { name: /Ada Lovelace/ }));
    expect(h.onaccount).toHaveBeenCalledOnce();
    expect(h.onclose).toHaveBeenCalledOnce();
  });

  it("account sheet closes before acting and cannot log out twice", async () => {
    const onclose = vi.fn();
    const onchoose = vi.fn();
    const { rerender } = render(AccountSheet, { open: true, displayName: "Ada", email: "ada@example.test", balance, signingOut: false, onclose, onchoose });
    await userEvent.click(screen.getByRole("button", { name: "Support" }));
    expect(onclose).toHaveBeenCalledOnce();
    expect(onchoose).toHaveBeenCalledWith("support");
    await rerender({ open: true, displayName: "Ada", email: "ada@example.test", balance, signingOut: true, onclose, onchoose });
    expect(screen.getByRole("button", { name: "Logout" })).toBeDisabled();
  });
});
