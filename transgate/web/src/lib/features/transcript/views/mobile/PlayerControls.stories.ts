import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import PlayerControls from "../shared/PlayerControls.svelte";
const meta = {
  title: "Features/Transcript/MobileViews/PlayerControls",
  component: PlayerControls,
  args: { title: fixture("TranscriptVm", "completed").title, state: { playing: false, time: 42, duration: 180 }, phone: true, expandedEnabled: true, ontoggle: fn(), onseek: fn(), onskip: fn(), onrate: fn() },
  parameters: { docs: { description: { component: "Pure phone controls, expandable without mounting a second engine. Snapshot values are not live playback evidence." } } },
} satisfies Meta<typeof PlayerControls>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Paused: Story = {};
export const Playing: Story = { args: { state: { playing: true, time: 42, duration: 180 } } };
export const ExpansionDisabled: Story = { args: { expandedEnabled: false } };
