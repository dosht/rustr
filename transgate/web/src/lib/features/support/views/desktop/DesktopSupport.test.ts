import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { SupportVm } from "$lib/core/types/SupportVm";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopSupport from "./DesktopSupport.svelte";

beforeAll(() => i18n.use("en"));

function setup(vm: SupportVm) {
  const intents = { onmessage: vi.fn(), onsubmit: vi.fn(), onclose: vi.fn() };
  render(DesktopSupport, { vm, ...intents });
  return intents;
}

describe("DesktopSupport", () => {
  it("sends the typed message through core", async () => {
    const intents = setup({ open: true, message: "", sending: false, notice: null });
    expect(screen.getByRole("dialog", { name: "Support" })).toBeVisible();
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "H");
    expect(intents.onmessage).toHaveBeenCalledWith("H");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(intents.onsubmit).toHaveBeenCalledOnce();
  });

  it("keeps the message and shows the failure notice (UI-04) instead of closing", () => {
    setup({ open: true, message: "Help with job 7", sending: false, notice: { ns: "app", key: "p5.support.send_failed", args: {} } as SupportVm["notice"] });
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Help with job 7");
    expect(screen.getByRole("alert")).toHaveTextContent(i18n.tk("app", "p5.support.send_failed"));
  });

  it("cannot be dismissed or resent while sending", async () => {
    const intents = setup({ open: true, message: "m", sending: true, notice: null });
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    screen.getByRole("dialog").dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(intents.onsubmit).not.toHaveBeenCalled();
    expect(intents.onclose).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
  });
});
