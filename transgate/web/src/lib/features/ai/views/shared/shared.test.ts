import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import { simpleMarkdown } from "../../markdown";
import AiConsentSettings from "./AiConsentSettings.svelte";

beforeAll(() => i18n.use("en"));

function settings(name: string) {
  const onintent = vi.fn();
  render(AiConsentSettings, { consent: fixture("ConsentPanelVm", name), onintent });
  return { onintent };
}

describe("AI consent settings", () => {
  it("distinguishes the privacy-policy link from surrounding text without hover", () => {
    settings("granted");
    const link = screen.getByRole("link", { name: i18n.tk("ai", "Privacy Policy") });
    expect(window.getComputedStyle(link).textDecorationLine.split(/\s+/)).toContain("underline");
  });

  it("enables from the disabled state", async () => {
    const { onintent } = settings("required");
    expect(screen.getByText("AI Features Disabled")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Enable AI Features" }));
    expect(onintent).toHaveBeenCalledWith({ type: "enable" });
  });

  // D22: the one destructive settings action names both effects and only
  // dispatches the revoking DELETE after its confirmation.
  it("turns AI off and deletes saved data only after the destructive confirmation", async () => {
    const { onintent } = settings("granted");
    const action = i18n.tk("app", "p5.consent.turn_off_delete_action");
    await userEvent.click(screen.getByRole("button", { name: action }));
    expect(onintent).not.toHaveBeenCalled();
    const confirm = screen.getByRole("alertdialog", { name: i18n.tk("app", "p5.consent.turn_off_delete_title") });
    expect(confirm).toHaveAccessibleDescription(i18n.tk("app", "p5.consent.turn_off_delete_confirm"));
    await userEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(onintent).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: action }));
    await userEvent.click(screen.getByRole("button", { name: i18n.tk("app", "p5.consent.turn_off_delete_button") }));
    expect(onintent).toHaveBeenCalledOnce();
    expect(onintent).toHaveBeenCalledWith({ type: "disable" });
  });

  // D21: a failed write keeps the controls and the confirmed state, with the
  // action's error inline; only a failed read replaces the tab.
  it("keeps AI enabled with an inline error and a usable control when the revoke failed", () => {
    settings("disable_failed");
    expect(screen.getByText("AI Features Enabled")).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent("Failed to disable AI features. Please try again.");
    expect(screen.getByRole("button", { name: i18n.tk("app", "p5.consent.turn_off_delete_action") })).toBeEnabled();
    expect(screen.queryByText(i18n.tk("app", "p5.consent.cleanup_pending"))).toBeNull();
  });

  it("reports a still-running account-side purge as pending, not done", () => {
    settings("cleanup_pending");
    expect(screen.getByText("AI Features Disabled")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent(i18n.tk("app", "p5.consent.cleanup_pending"));
  });

  it("shows only the read error when consent could not be loaded", () => {
    settings("load_failed");
    expect(screen.getByRole("alert")).toHaveTextContent("Failed to load consent preferences");
    expect(screen.queryByRole("button")).toBeNull();
  });
});

describe("simpleMarkdown", () => {
  it("escapes answer HTML before formatting", () => {
    expect(simpleMarkdown('<img src=x onerror="a()"> **bold**')).toBe("&lt;img src=x onerror=&quot;a()&quot;&gt; <strong>bold</strong>");
  });

  it("groups consecutive list lines and formats headings and italics", () => {
    expect(simpleMarkdown("# Plan\n- one\n2. two\n*done*")).toBe("<h3>Plan</h3>\n<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n<em>done</em>");
  });
});
