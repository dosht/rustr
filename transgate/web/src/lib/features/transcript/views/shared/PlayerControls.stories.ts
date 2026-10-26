import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import PlayerControls from "./PlayerControls.svelte";
const meta = {
  title: "Features/Transcript/DesktopViews/PlayerControls",
  component: PlayerControls,
  args: { title: fixture("TranscriptVm", "completed").title, state: { playing: false, time: 0, duration: null }, ontoggle: fn(), onseek: fn(), onskip: fn(), onrate: fn() },
  parameters: { docs: { description: { component: "UI control snapshots only. Engine playhead values illustrate states; no audio/provider is mounted and no playback is claimed." } } },
} satisfies Meta<typeof PlayerControls>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Waiting: Story = { args: { disabled: true } };
export const Paused: Story = { args: { state: { playing: false, time: 42, duration: 180 } } };
export const Playing: Story = { args: { state: { playing: true, time: 42, duration: 180 } } };
