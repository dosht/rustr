import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import MobileRecent from "./MobileRecent.svelte";
const meta = { title: "Features/History/MobileViews/MobileRecent", component: MobileRecent, args: { onopen: fn() } } satisfies Meta<typeof MobileRecent>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Recent: Story = { args: { vm: fixture("JobListVm", "sample") } };
export const Empty: Story = { args: { vm: fixture("JobListVm", "empty") } };
