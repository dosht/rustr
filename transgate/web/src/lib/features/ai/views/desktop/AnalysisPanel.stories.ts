import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import AnalysisPanel from "./AnalysisPanel.svelte";

const granted = fixture("ConsentPanelVm", "granted");
const state = (name: string) => ({ args: { view: fixture("AnalysisPanelVm", name) } });

const meta = {
  title: "Features/AI/DesktopViews/AnalysisPanel",
  component: AnalysisPanel,
  parameters: { viewport: { defaultViewport: "desktop" } },
  args: { consent: granted, view: fixture("AnalysisPanelVm", "meeting"), onconsent: fn(), onintent: fn() },
} satisfies Meta<typeof AnalysisPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ConsentLoading: Story = { args: { consent: fixture("ConsentPanelVm", "loading") } };
export const Introduction: Story = { args: { consent: fixture("ConsentPanelVm", "required") } };
export const Starting: Story = state("starting");
export const Streaming: Story = state("streaming");
export const Generic: Story = state("generic");
export const Meeting: Story = state("meeting");
export const Lecture: Story = state("lecture");
export const Sales: Story = state("sales");
export const Support: Story = state("support");
export const Interview: Story = state("interview");
export const Interrupted: Story = state("interrupted");
export const Rejected: Story = state("rejected");
export const Unconfirmed: Story = state("unconfirmed");
export const Exporting: Story = state("exporting");
export const ExportFailed: Story = state("export_failed");
export const NoTranscript: Story = state("no_transcript");
