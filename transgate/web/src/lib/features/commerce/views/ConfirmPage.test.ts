import { render, screen } from "@testing-library/svelte";
import { beforeAll, describe, expect, it } from "vitest";
import { fixture } from "$lib/core/fixtures";
import { i18n } from "$lib/i18n/index.svelte";
import ConfirmPage from "./ConfirmPage.svelte";

beforeAll(() => i18n.use("en"));

describe("confirmation outcome rendering", () => {
  it("unconfirmed shows one warning and no success/failure heading or countdown", () => {
    const messages = fixture("ConfirmMessages", "pending");
    render(ConfirmPage, { messages });
    expect(screen.getAllByText(i18n.t(messages.message))).toHaveLength(1);
    expect(screen.queryByRole("heading", { level: 2 })).toBeNull();
    expect(screen.queryByText(/redirected/)).toBeNull();
  });

  it("a definitive outcome renders core's countdown and removes it when unconfirmed", async () => {
    const messages = fixture("ConfirmMessages", "counting_down");
    const view = render(ConfirmPage, { messages });
    if (!messages.redirect) throw new Error("countdown fixture lacks redirect message");
    expect(screen.getByText(i18n.t(messages.redirect))).toBeInTheDocument();
    await view.rerender({ messages: fixture("ConfirmMessages", "unconfirmed") });
    expect(screen.queryByText(i18n.t(messages.redirect))).toBeNull();
  });
});
