import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import AiConsentSettings from "./AiConsentSettings.svelte";

// One responsive view, as the source AIConsentView was; the settings page
// mounts it in both its desktop and phone layouts.
const state = (name: string) => ({ args: { consent: fixture("ConsentPanelVm", name) } });

const meta = {
  title: "Features/AI/DesktopViews/AiConsentSettings",
  component: AiConsentSettings,
  args: { consent: fixture("ConsentPanelVm", "granted"), onintent: fn() },
} satisfies Meta<typeof AiConsentSettings>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Loading: Story = state("loading");
export const Disabled: Story = state("required");
export const Enabled: Story = state("granted");
export const Enabling: Story = state("enabling");
export const DisableFailed: Story = state("disable_failed");
export const CleanupPending: Story = state("cleanup_pending");
export const LoadFailed: Story = state("load_failed");
export const Phone: Story = { ...state("granted"), parameters: { viewport: { defaultViewport: "pixel7" } } };
