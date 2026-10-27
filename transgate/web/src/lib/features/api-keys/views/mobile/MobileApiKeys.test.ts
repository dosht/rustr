import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import MobileApiKeys from "./MobileApiKeys.svelte";

beforeAll(() => i18n.use("en"));
const props = (name: string) => ({ vm: fixture("ApiKeysVm", name), onintent: vi.fn() });
const rowOf = (name: string) => screen.getByText(name).closest("article") as HTMLElement;

describe("Mobile API keys", () => {
  it("shows each key with core's labels, the localized created time and revoke only where core allows it", () => {
    render(MobileApiKeys, props("list"));
    const owner = within(rowOf("CI pipeline"));
    expect(owner.getByText("Read Only")).toBeInTheDocument();
    expect(owner.getByText("Create Only")).toBeInTheDocument();
    expect(owner.getByText("Never")).toBeInTheDocument();
    expect(owner.getByText(new Date(1766361600000).toLocaleString())).toBeInTheDocument();
    expect(within(rowOf("Reporting")).getByText("3 Days")).toBeInTheDocument();
    const old = within(rowOf("Old integration"));
    expect(old.getByText("Expired")).toBeInTheDocument();
    expect(old.getByText("Revoked")).toBeInTheDocument();
    expect(old.queryByRole("button", { name: "Revoke" })).toBeNull();
    expect(owner.getByRole("button", { name: "Revoke" })).toBeEnabled();
  });

  it("shows the key once with copy, the signing secret and the dismissible notice, forwarding intents", async () => {
    const created = props("created");
    render(MobileApiKeys, created);
    expect(screen.getByText("sk_q1W2e3R4t5Y6u7I8o9P0a1S2d3F4g5H6j7K8l9Z0x1C=")).toBeInTheDocument();
    expect(screen.getByText("whsec_7f3a9c2e41b8d6")).toBeInTheDocument();
    const keyCopy = screen.getByRole("button", { name: "Copied!" });
    const secretCopy = screen.getByRole("button", { name: "Copy secret" });
    expect(screen.getByRole("heading", { name: "Callback signing secret" })).toBeInTheDocument();
    await userEvent.click(keyCopy);
    expect(created.onintent).toHaveBeenLastCalledWith({ type: "copy_requested", what: "key" });
    await userEvent.click(secretCopy);
    expect(created.onintent).toHaveBeenLastCalledWith({ type: "copy_requested", what: "secret" });
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(created.onintent).toHaveBeenLastCalledWith({ type: "notice_dismissed" });
  });

  it("keeps the secret and keys out of the page when core withholds them, and shows core's load errors", () => {
    render(MobileApiKeys, props("load_failed"));
    expect(screen.queryByText(/whsec_/)).toBeNull();
    expect(screen.queryByText(/sk_/)).toBeNull();
    expect(screen.getByText("Failed to load API keys.")).toBeInTheDocument();
    expect(screen.getByText("Failed to load the signing secret.")).toBeInTheDocument();
    // Source parity: an empty list after a failed load shows the empty message, never a bare table or header.
    expect(screen.getByText("No API keys found.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("columnheader")).toBeNull();
  });

  it("renders loading and empty states from core's flags", async () => {
    const view = render(MobileApiKeys, props("loading"));
    expect(screen.getByRole("status", { name: "" })).toHaveAttribute("aria-busy", "true");
    await view.rerender(props("empty"));
    expect(screen.getByText("No API keys found.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("forwards form edits as typed intents and disables nothing core allows", async () => {
    const form = props("creating_form");
    render(MobileApiKeys, form);
    expect(screen.getByRole("textbox", { name: "Key Name" })).toHaveValue("Nightly export");
    expect(screen.getByRole("combobox", { name: "Expiration Policy" })).toHaveValue("custom");
    expect(screen.getByRole("textbox", { name: "Custom Days" })).toHaveValue("30");
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Permissions" }), "create");
    expect(form.onintent).toHaveBeenLastCalledWith({ type: "scope_chosen", scope: "create" });
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Expiration Policy" }), "90_days");
    expect(form.onintent).toHaveBeenLastCalledWith({ type: "policy_chosen", policy: "90_days" });
    await userEvent.type(screen.getByRole("textbox", { name: "Custom Days" }), "5");
    expect(form.onintent).toHaveBeenLastCalledWith({ type: "custom_days_changed", text: "305" });
    await userEvent.click(screen.getByRole("button", { name: "Generate Key" }));
    expect(form.onintent).toHaveBeenLastCalledWith({ type: "create_requested" });
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(form.onintent).toHaveBeenLastCalledWith({ type: "form_toggled" });
  });

  it("asks core to revoke, and confirms or cancels only through the dialog", async () => {
    const list = props("list");
    const view = render(MobileApiKeys, list);
    await userEvent.click(within(rowOf("CI pipeline")).getByRole("button", { name: "Revoke" }));
    expect(list.onintent).toHaveBeenLastCalledWith({ type: "revoke_requested", id: "00000000-0000-0000-0000-000000000001" });
    const confirm = props("confirm_revoke");
    await view.rerender(confirm);
    const dialog = screen.getByRole("dialog", { name: "Revoke API key" });
    expect(within(dialog).getByText("Are you sure you want to revoke this API key?")).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));
    expect(confirm.onintent).toHaveBeenLastCalledWith({ type: "revoke_confirmed" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(confirm.onintent).toHaveBeenLastCalledWith({ type: "revoke_canceled" });
  });
});
