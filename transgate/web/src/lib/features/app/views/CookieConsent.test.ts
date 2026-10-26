import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { i18n } from "$lib/i18n/index.svelte";
import CookieConsent from "./CookieConsent.svelte";

beforeAll(() => i18n.use("en"));

describe("CookieConsent", () => {
  it("asks until answered and reports the answer", async () => {
    const onaccept = vi.fn();
    const onreject = vi.fn();
    const { rerender } = render(CookieConsent, { open: true, onaccept, onreject });
    expect(screen.getByRole("region", { name: /We use cookies/ })).toBeVisible();
    expect(screen.getByRole("link", { name: "Learn more" })).toHaveAttribute("href", "/legal/privacy-policy-en");
    await userEvent.click(screen.getByRole("button", { name: "Reject" }));
    expect(onreject).toHaveBeenCalledOnce();
    expect(onaccept).not.toHaveBeenCalled();

    await rerender({ open: false, onaccept, onreject });
    expect(screen.queryByRole("region")).toBeNull();
  });
});
