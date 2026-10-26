import { createRawSnippet } from "svelte";
import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import Button from "./Button.svelte";
import ConfirmDialog from "./ConfirmDialog.svelte";
import Menu from "./Menu.svelte";
import Progress from "./Progress.svelte";
import Switch from "./Switch.svelte";
import Tabs from "./Tabs.svelte";
import TextField from "./TextField.svelte";

const text = (value: string) => createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

describe("Button", () => {
  it("ignores clicks while busy but stays focusable and announces it", async () => {
    const onclick = vi.fn();
    render(Button, { busy: true, onclick, children: text("Pay") });
    const button = screen.getByRole("button", { name: "Pay" });
    await userEvent.click(button);
    expect(onclick).not.toHaveBeenCalled();
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).not.toBeDisabled();
  });
});

describe("Switch", () => {
  it("reports the requested value and leaves the decision to the owner", async () => {
    const onchange = vi.fn();
    render(Switch, { checked: true, label: "Automatic cleanup", description: "Deletes after 30 days", onchange });
    const control = screen.getByRole("switch", { name: "Automatic cleanup" });
    expect(control).toHaveAccessibleDescription("Deletes after 30 days");
    await userEvent.click(control);
    expect(onchange).toHaveBeenCalledWith(false);
    expect(control).toHaveAttribute("aria-checked", "true");
  });
});

describe("ConfirmDialog", () => {
  it("confirms once and cannot be dismissed while the action is in flight", async () => {
    const onconfirm = vi.fn();
    const oncancel = vi.fn();
    const props = { open: true, title: "Delete account?", description: "This cannot be undone.", confirmLabel: "Delete", cancelLabel: "Cancel", tone: "danger" as const, onconfirm, oncancel };
    const { rerender } = render(ConfirmDialog, props);
    const dialog = screen.getByRole("dialog", { name: "Delete account?" });
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onconfirm).toHaveBeenCalledOnce();

    await rerender({ ...props, busy: true });
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(onconfirm).toHaveBeenCalledOnce();
    expect(oncancel).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("Escape asks the owner to cancel when idle", async () => {
    const oncancel = vi.fn();
    render(ConfirmDialog, { open: true, title: "Sign out?", description: "d", confirmLabel: "Sign out", cancelLabel: "Stay", onconfirm: vi.fn(), oncancel });
    screen.getByRole("dialog").dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(oncancel).toHaveBeenCalledOnce();
  });
});

describe("Tabs", () => {
  it("moves selection and focus with arrows, Home and End", async () => {
    const onselect = vi.fn();
    const tabs = [{ id: "transcript", label: "Transcript" }, { id: "summary", label: "Summary" }, { id: "chat", label: "Chat" }] as const;
    render(Tabs, { label: "Job views", tabs, selected: "transcript", onselect, panel: createRawSnippet((id: () => string) => ({ render: () => `<p>${id()}</p>` })) });
    expect(screen.getByRole("tabpanel")).toHaveAccessibleName("Transcript");
    screen.getByRole("tab", { name: "Transcript" }).focus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(onselect).toHaveBeenLastCalledWith("chat");
    expect(screen.getByRole("tab", { name: "Chat" })).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(onselect).toHaveBeenLastCalledWith("transcript");
  });
});

describe("Menu", () => {
  it("is a disclosure: opens, selects, closes and returns focus; Escape closes", async () => {
    const onselect = vi.fn();
    render(Menu, { label: "Account", items: [{ id: "settings", label: "Settings" }, { id: "signout", label: "Sign out" }], onselect });
    const trigger = screen.getByRole("button", { name: "Account" });
    await userEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
    expect(onselect).toHaveBeenCalledWith("signout");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(trigger).toHaveFocus();

    await userEvent.click(trigger);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "Settings" })).toBeNull();
    expect(trigger).toHaveFocus();
  });
});

describe("TextField", () => {
  it("labels the field and announces its error", async () => {
    const oninput = vi.fn();
    render(TextField, { label: "Email", value: "", type: "email", error: "Enter a valid email", oninput });
    const field = screen.getByRole("textbox", { name: "Email" });
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveAccessibleDescription("Enter a valid email");
    await userEvent.type(field, "a");
    expect(oninput).toHaveBeenCalledWith("a");
  });
});

describe("Progress", () => {
  it("clamps a known value and omits it when indeterminate", async () => {
    const { rerender } = render(Progress, { label: "Uploading", value: 140 });
    expect(screen.getByRole("progressbar", { name: "Uploading" })).toHaveAttribute("aria-valuenow", "100");
    await rerender({ label: "Uploading", value: null });
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
  });
});
