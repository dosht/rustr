import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import SpecialOffer from "./SpecialOffer.svelte";

beforeAll(() => i18n.use("en"));
function offer() {
  const offer = fixture("SignedInVm", "special_offer").special_client_alert;
  if (!offer) throw new Error("special offer fixture has no offer");
  return offer;
}

describe("configured special offer", () => {
  it("renders the host message literally, never as HTML or a re-derived percentage", () => {
    const message = '<img src="x" onerror="alert(1)"> A host-defined discount.';
    const { container } = render(SpecialOffer, { offer: { ...offer(), message }, ondismiss: vi.fn(), oncopy: vi.fn() });
    expect(screen.getByText(message)).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).not.toContain("25%");
  });

  it("does not claim the code was copied when the clipboard refused it", async () => {
    render(SpecialOffer, { offer: offer(), ondismiss: vi.fn(), oncopy: vi.fn().mockResolvedValue(false) });
    await userEvent.click(screen.getByRole("button", { name: "Copy Code" }));
    expect(screen.queryByRole("button", { name: "Copied" })).toBeNull();
    expect(screen.getByRole("button", { name: "Copy Code" })).toBeEnabled();
  });
});
