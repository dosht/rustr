// One root-scoped PDF resource. Core fences async document delivery; this store
// only owns the browser URL and releases it when its visible selection changes.
import type { AppVm } from "$lib/core/types/AppVm";

export class InvoicePreview {
  url: string | null = $state(null);
  filename = $state("");
  #selection: string | null = null;
  readonly #urls: Pick<typeof URL, "createObjectURL" | "revokeObjectURL">;

  constructor(urls: Pick<typeof URL, "createObjectURL" | "revokeObjectURL"> = URL) {
    this.#urls = urls;
  }

  sync(view: AppVm): void {
    const invoice = view.state === "signed_in" && view.route.page === "billing" ? view.invoices : null;
    this.#select(view.state === "signed_in" && invoice?.preview != null
      ? JSON.stringify([view.profile?.account, invoice.admin?.viewing_customer, invoice.admin?.temporary_bill_to, invoice.preview])
      : null);
  }
  #select(selection: string | null): void {
    if (selection === this.#selection) return;
    this.clear();
    this.#selection = selection;
  }

  present(bytes: Uint8Array, filename: string, mime: string): void {
    // A missing surface must not create an unowned URL. Normally the Rust
    // delivery predicate rejects this before calling Platform.preview.
    if (this.#selection === null) throw new Error("invoice preview is closed");
    const next = this.#urls.createObjectURL(new Blob([bytes as Uint8Array<ArrayBuffer>], { type: mime }));
    this.clear();
    this.filename = filename;
    this.url = next;
  }

  clear(): void {
    if (this.url !== null) this.#urls.revokeObjectURL(this.url);
    this.url = null;
    this.filename = "";
  }

  dispose(): void {
    this.clear();
    this.#selection = null;
  }
}
