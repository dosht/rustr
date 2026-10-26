import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopSubscription from "./DesktopSubscription.svelte";

beforeAll(() => i18n.use("en"));
function props(name: string) {
  return { vm: fixture("SubscriptionVm", name), messages: fixture("BillingMessages", `subscription_${name}`), onintent: vi.fn() };
}

describe("Desktop subscription", () => {
  it("replaces cancellation with the keep-minutes notice only after core reports cancellation", async () => {
    const view = render(DesktopSubscription, props("active"));
    expect(screen.getByRole("button", { name: "Cancel Subscription" })).toBeInTheDocument();
    await view.rerender(props("canceled"));
    expect(screen.queryByRole("button", { name: "Cancel Subscription" })).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Your subscription is canceled, but you can still use your minutes.");
  });

  it("cannot confirm again or escape while cancellation is in flight", async () => {
    const initial = props("cancelling");
    render(DesktopSubscription, initial);
    await userEvent.click(screen.getByRole("button", { name: "Yes" }));
    screen.getByRole("dialog").dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(initial.onintent).not.toHaveBeenCalled();
  });
});
