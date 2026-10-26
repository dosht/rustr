import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import ChatPanel from "./ChatPanel.svelte";

const state = (name: string) => ({ args: { view: fixture("ChatPanelVm", name) } });

const meta = {
  title: "Features/AI/MobileViews/ChatPanel",
  component: ChatPanel,
  parameters: { viewport: { defaultViewport: "pixel7" } },
  args: { consent: fixture("ConsentPanelVm", "granted"), view: fixture("ChatPanelVm", "history"), onconsent: fn(), onintent: fn() },
} satisfies Meta<typeof ChatPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Introduction: Story = { args: { consent: fixture("ConsentPanelVm", "required") } };
export const LoadingHistory: Story = state("loading_history");
export const Empty: Story = state("welcome");
export const History: Story = state("history");
export const Typing: Story = state("typing");
export const Streaming: Story = state("streaming");
export const SyncWarning: Story = state("sync_warning");
export const LimitReached: Story = state("limit_reached");
export const Copied: Story = state("copied");
