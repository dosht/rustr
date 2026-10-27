import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import ChatPanel from "./ChatPanel.svelte";

const state = (name: string) => ({ args: { view: fixture("ChatPanelVm", name) } });

const meta = {
  title: "Features/AI/DesktopViews/ChatPanel",
  component: ChatPanel,
  parameters: { viewport: { defaultViewport: "desktop" } },
  args: { consent: fixture("ConsentPanelVm", "granted"), view: fixture("ChatPanelVm", "history"), onconsent: fn(), onintent: fn() },
} satisfies Meta<typeof ChatPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Introduction: Story = { args: { consent: fixture("ConsentPanelVm", "required") } };
export const LoadingHistory: Story = state("loading_history");
export const Welcome: Story = state("welcome");
export const History: Story = state("history");
export const Typing: Story = state("typing");
export const Streaming: Story = state("streaming");
export const Reattached: Story = state("reattached");
export const SyncWarning: Story = state("sync_warning");
export const LimitReached: Story = state("limit_reached");
export const Disabled: Story = state("disabled");
export const Copied: Story = state("copied");
export const WithDraft: Story = { args: { draft: "What did Ada promise?" } };
