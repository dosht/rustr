import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import fixtures from "$lib/core/fixtures.json";
import type { AuthVm } from "$lib/core/types/AuthVm";
import AuthView from "./AuthView.svelte";

const vms = fixtures.AuthVm as Record<string, AuthVm>;

// The Turnstile widget needs a live site key and network, so stories pass none;
// `CaptchaRequired` shows core's unsubmittable state without the widget.
const meta = {
  title: "Pages/App/SignIn",
  component: AuthView,
  args: { captchaSiteKey: null, onintent: fn() },
} satisfies Meta<typeof AuthView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const SignIn: Story = { args: { vm: vms.sign_in } };
export const SignUp: Story = { args: { vm: vms.sign_up } };
export const MagicLink: Story = { args: { vm: vms.magic_link } };
export const ForgottenPassword: Story = { args: { vm: vms.forgotten_password } };
export const Busy: Story = { args: { vm: vms.busy } };
export const Notice: Story = { args: { vm: vms.notice } };
export const Failed: Story = { args: { vm: vms.error } };
export const CaptchaRequired: Story = { args: { vm: vms.captcha_required } };
