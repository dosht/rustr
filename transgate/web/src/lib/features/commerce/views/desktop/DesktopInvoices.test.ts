import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopInvoices from "./DesktopInvoices.svelte";

beforeAll(() => i18n.use("en"));
function props(name: string) {
  return { vm: fixture("InvoicesVm", name), messages: fixture("BillingMessages", `invoices_${name}`), onintent: vi.fn(), onback: vi.fn(), previewUrl: null as string | null };
}

describe("Desktop invoices", () => {
  it("does not expose customer lookup to an owner, but preserves admin input when available", async () => {
    const view = render(DesktopInvoices, props("owner"));
    expect(screen.queryByRole("textbox", { name: "Look up customer invoices by email" })).toBeNull();
    await view.rerender(props("admin_customer"));
    expect(screen.getByRole("textbox", { name: "Custom Bill To (tax info, address, company)" })).toHaveValue("Grace Ltd\nLondon");
  });

  it("shows the returned PDF inline and closes by intent without opening a window", async () => {
    const initial = { ...props("preview"), previewUrl: "blob:invoice-preview" };
    const open = vi.spyOn(window, "open");
    const view = render(DesktopInvoices, initial);
    expect(screen.getByTitle("Invoice Preview")).toHaveAttribute("src", "blob:invoice-preview");
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(initial.onintent).toHaveBeenCalledWith({ type: "close_preview" });
    expect(open).not.toHaveBeenCalled();
    await view.rerender(props("owner"));
    expect(screen.queryByTitle("Invoice Preview")).toBeNull();
    open.mockRestore();
  });

  it("keeps invoice actions available after document failure, and replaces the PDF with the error", () => {
    const initial = { ...props("document_failed"), previewUrl: "blob:obsolete" };
    render(DesktopInvoices, initial);
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.queryByTitle("Invoice Preview")).toBeNull();
    expect(screen.getAllByRole("button", { name: "Download" }).every((button) => !button.hasAttribute("disabled"))).toBe(true);
  });
});
