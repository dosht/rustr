import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { AuthVm } from "$lib/core/types/AuthVm";
import { i18n } from "$lib/i18n/index.svelte";
import AuthView from "./AuthView.svelte";

function vm(patch: Partial<AuthVm> = {}): AuthVm {
  return { mode: "sign_in", email: "", password: "", captcha_required: false, busy: false, can_submit: true, error: null, notice: null, ...patch };
}

beforeAll(() => i18n.use("en"));

describe("AuthView", () => {
  it("sign in: Google, email/password, submit and the three source links as intents", async () => {
    const onintent = vi.fn();
    render(AuthView, { vm: vm(), captchaSiteKey: null, onintent });
    await userEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "google_chosen" });
    await userEvent.type(screen.getByRole("textbox", { name: "Email address" }), "a");
    expect(onintent).toHaveBeenLastCalledWith({ type: "email_changed", text: "a" });
    expect(screen.getByLabelText("Your Password")).toHaveAttribute("type", "password");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
    // Native form constraints must prevent submission of the incomplete draft.
    expect(onintent).not.toHaveBeenCalledWith({ type: "submitted" });
    await userEvent.clear(screen.getByRole("textbox", { name: "Email address" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Email address" }), "ada@example.test");
    await userEvent.type(screen.getByLabelText("Your Password"), "example-password");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "submitted" });
    await userEvent.click(screen.getByRole("button", { name: "Don't have an account? Sign up" }));
    expect(onintent).toHaveBeenLastCalledWith({ type: "mode_chosen", mode: "sign_up" });
    expect(screen.getByRole("button", { name: "Forgot your password?" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send a magic link email" })).toBeInTheDocument();
  });

  it("magic link: email only, no social sign-in, back to sign in", () => {
    render(AuthView, { vm: vm({ mode: "magic_link" }), captchaSiteKey: null, onintent: vi.fn() });
    expect(screen.queryByLabelText(/password/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /Google/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Send Magic Link" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Already have an account? Sign in" })).toBeInTheDocument();
  });

  it("busy shows the source loading label and core's notice and error", () => {
    render(AuthView, {
      vm: vm({ mode: "sign_up", busy: true, notice: { ns: "auth", key: "sign_up.confirmation_text", args: {} }, error: { ns: "common", key: "Loading...", args: {} } } as Partial<AuthVm>),
      captchaSiteKey: null,
      onintent: vi.fn(),
    });
    expect(screen.getByRole("button", { name: "Signing up ..." })).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Check your email for the confirmation link");
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  it("renders the captcha slot only when core requires it and a site key exists", async () => {
    const { rerender } = render(AuthView, { vm: vm({ captcha_required: true }), captchaSiteKey: null, onintent: vi.fn() });
    expect(screen.queryByTestId("turnstile")).toBeNull();
    await rerender({ vm: vm({ captcha_required: false }), captchaSiteKey: "site", onintent: vi.fn() });
    expect(screen.queryByTestId("turnstile")).toBeNull();
  });
});
