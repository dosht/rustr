import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import Menu from "./Menu.svelte";

const meta = {
  title: "UI/Primitives/Menu",
  component: Menu,
  args: { label: "Account", items: [{ id: "settings", label: "Settings" }, { id: "signout", label: "Sign out" }], onselect: fn() },
} satisfies Meta<typeof Menu>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Closed: Story = { args: {} };
