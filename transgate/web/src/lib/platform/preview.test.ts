import { describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import type { AppVm } from "$lib/core/types/AppVm";
import { InvoicePreview } from "./preview.svelte";

function resource() {
  const createObjectURL = vi.fn<(blob: Blob) => string>().mockReturnValueOnce("blob:first").mockReturnValueOnce("blob:second");
  const revokeObjectURL = vi.fn();
  return { preview: new InvoicePreview({ createObjectURL, revokeObjectURL }), createObjectURL, revokeObjectURL };
}

function openInvoice(): AppVm {
  const app = fixture("AppVm", "billing_owner");
  if (app.state !== "signed_in") throw new Error("billing fixture must be signed in");
  return { ...app, invoices: fixture("InvoicesVm", "preview") };
}

const bytes = new TextEncoder().encode("%PDF-1.7\nsynthetic preview bytes");

describe("invoice preview URL ownership", () => {
  it("replaces document bytes and revokes each URL exactly once", async () => {
    const { preview, createObjectURL, revokeObjectURL } = resource();
    preview.sync(openInvoice());
    preview.present(bytes, "invoice.pdf", "application/pdf");
    expect(preview.url).toBe("blob:first");
    expect(createObjectURL.mock.calls[0][0].type).toBe("application/pdf");
    expect(createObjectURL.mock.calls[0][0].size).toBe(bytes.byteLength);
    preview.present(bytes, "replacement.pdf", "application/pdf");
    expect(preview.url).toBe("blob:second");
    expect(preview.filename).toBe("replacement.pdf");
    expect(revokeObjectURL.mock.calls).toEqual([["blob:first"]]);
    preview.dispose();
    preview.dispose();
    expect(preview.url).toBeNull();
    expect(revokeObjectURL.mock.calls).toEqual([["blob:first"], ["blob:second"]]);
  });

  it.each(["billing_owner", "desktop_new", "signed_out", "erased"])("releases the PDF on %s and rejects delivery without an open preview", (name) => {
    const { preview, createObjectURL, revokeObjectURL } = resource();
    preview.sync(openInvoice());
    preview.present(bytes, "invoice.pdf", "application/pdf");
    preview.sync(fixture("AppVm", name));
    expect(preview.url).toBeNull();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:first");
    expect(() => preview.present(bytes, "late.pdf", "application/pdf")).toThrow("invoice preview is closed");
    expect(createObjectURL).toHaveBeenCalledTimes(1);
  });

  it("reopening the same payment starts without the previous PDF", () => {
    const { preview } = resource();
    preview.sync(openInvoice());
    preview.present(bytes, "invoice.pdf", "application/pdf");
    preview.sync(fixture("AppVm", "billing_owner"));
    preview.sync(openInvoice());
    expect(preview.url).toBeNull();
    preview.present(bytes, "fresh.pdf", "application/pdf");
    expect(preview.url).toBe("blob:second");
  });

  it("propagates URL creation failure without revoking the still-owned resource", () => {
    const { preview, createObjectURL, revokeObjectURL } = resource();
    preview.sync(openInvoice());
    preview.present(bytes, "invoice.pdf", "application/pdf");
    createObjectURL.mockReset().mockImplementationOnce(() => { throw new Error("URL unavailable"); });
    expect(() => preview.present(bytes, "new.pdf", "application/pdf")).toThrow("URL unavailable");
    expect(preview.url).toBe("blob:first");
    expect(revokeObjectURL).not.toHaveBeenCalled();
    preview.dispose();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:first");
  });
});
