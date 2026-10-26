import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import DesktopPurchase from "./DesktopPurchase.svelte";

beforeAll(() => i18n.use("en"));
const callbacks = () => ({ onintent: vi.fn(), onclose: vi.fn(), ondismissoffer: vi.fn(), oncopy: vi.fn().mockResolvedValue(true) });
function props(name: string) {
  return { vm: fixture("PurchaseVm", name), messages: fixture("BillingMessages", `purchase_${name}`), ...callbacks() };
}

describe("Desktop purchase", () => {
  it("uses the shown total, not the distinct provider charge, and blocks payment until core permits it", async () => {
    const initial = props("display_differs_from_charge");
    const view = render(DesktopPurchase, initial);
    expect(screen.getByText("$7.40")).toBeInTheDocument();
    expect(screen.queryByText("$7.45")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Pay with Card" }));
    expect(initial.onintent).not.toHaveBeenCalled();
    await view.rerender(props("ready_to_pay"));
    expect(screen.getByRole("button", { name: "Pay with Card" })).toBeEnabled();
  });

  it("does not submit a checkout when Enter applies a coupon", async () => {
    const initial = props("coupon_expired");
    render(DesktopPurchase, initial);
    const coupon = screen.getByRole("textbox", { name: "Coupon" });
    await userEvent.click(coupon);
    await userEvent.keyboard("{Enter}");
    expect(initial.onintent).toHaveBeenCalledWith({ type: "coupon_applied" });
    expect(initial.onintent).not.toHaveBeenCalledWith({ type: "pay_requested" });
  });

  it("prevents repeat payment and dismissal while processing", async () => {
    const initial = props("processing");
    render(DesktopPurchase, initial);
    await userEvent.click(screen.getByRole("button", { name: "Pay with Card" }));
    const dialog = screen.getByRole("dialog");
    dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
    expect(initial.onintent).not.toHaveBeenCalled();
    expect(initial.onclose).not.toHaveBeenCalled();
  });

  it("keeps both selection sections available on desktop", () => {
    render(DesktopPurchase, props("pay_as_you_go"));
    expect(screen.getByRole("textbox", { name: "Credit Amount" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Yearly" })).toBeInTheDocument();
  });
});
