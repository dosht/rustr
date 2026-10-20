import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { signIn } from "../../core/fixtures";
import SignInView from "./SignInView.svelte";

const meta = {
  title: "Features/App/SignIn",
  component: SignInView,
  args: { onIntent: fn(), devSignIn: false, view: signIn("sign-in") },
} satisfies Meta<typeof SignInView>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
export const WrongPassword: Story = { args: { view: signIn("sign-in-error") } };
export const DevSignIn: Story = { args: { devSignIn: true } };
export const Phone: Story = { globals: { viewport: { value: "mobile1", isRotated: false } } };
