import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { signIn } from "../../core/fixtures";
import SignInView from "./SignInView.svelte";

describe("SignInView", () => {
  it("signs in or creates an account with what was typed", async () => {
    const onIntent = vi.fn();
    render(SignInView, { view: signIn("sign-in"), onIntent, devSignIn: false });
    await userEvent.type(screen.getByLabelText("Email"), "ada@example.com");
    await userEvent.type(screen.getByLabelText("Password"), "secret123");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(onIntent).toHaveBeenLastCalledWith({
      type: "SignInSubmitted",
      email: "ada@example.com",
      password: "secret123",
    });
    await userEvent.click(screen.getByRole("button", { name: "Create account" }));
    expect(onIntent).toHaveBeenLastCalledWith({
      type: "SignUpSubmitted",
      email: "ada@example.com",
      password: "secret123",
    });
  });

  it("shows the core's message for a failed sign-in", () => {
    render(SignInView, { view: signIn("sign-in-error"), onIntent: vi.fn(), devSignIn: false });
    expect(screen.getByRole("alert")).toHaveTextContent("Wrong email or password.");
  });

  it("asks for no password with dev sign-in", () => {
    render(SignInView, { view: signIn("sign-in"), onIntent: vi.fn(), devSignIn: true });
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create account" })).not.toBeInTheDocument();
  });
});
