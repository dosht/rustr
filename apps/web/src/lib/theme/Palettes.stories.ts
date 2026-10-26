import type { Meta, StoryObj } from "@storybook/svelte";
import { fn } from "storybook/test";
import PalettePreview from "./PalettePreview.svelte";
import { palettes, defaultPalette, modes } from "./palettes";

const meta = {
  title: "Pages/App/Palettes",
  component: PalettePreview,
  args: {
    palette: defaultPalette,
    mode: "light",
    platform: "desktop",
    screen: "tasks",
    showError: false,
    readOnly: false,
    onIntent: fn(),
  },
  argTypes: {
    palette: { control: "select", options: palettes.map((palette) => palette.id) },
    mode: { control: "inline-radio", options: modes },
    platform: { control: "inline-radio", options: ["desktop", "phone"] },
    screen: { control: "inline-radio", options: ["tasks", "sign-in", "members"] },
    showError: { control: "boolean", description: "Use the generated sign-in-error fixture." },
    readOnly: { control: "boolean", description: "Use the generated viewer task/member fixture." },
  },
} satisfies Meta<typeof PalettePreview>;

export default meta;
type Story = StoryObj<typeof meta>;

function preview(
  palette: (typeof palettes)[number]["id"],
  mode: (typeof modes)[number],
  platform: "desktop" | "phone",
  screen: "tasks" | "sign-in" | "members",
): Story {
  return { args: { palette, mode, platform, screen } };
}

// Explicit stories make all 3 x 2 x 2 x 3 comparisons addressable and inspectable.
// One screen per canvas avoids competing global keyboard handlers and fixed bars.
export const MossLightDesktopTasks = preview("paper-moss", "light", "desktop", "tasks");
export const MossDarkDesktopTasks = preview("paper-moss", "dark", "desktop", "tasks");
export const IndigoLightDesktopTasks = preview("paper-indigo", "light", "desktop", "tasks");
export const IndigoDarkDesktopTasks = preview("paper-indigo", "dark", "desktop", "tasks");
export const OchreLightDesktopTasks = preview("paper-ochre", "light", "desktop", "tasks");
export const OchreDarkDesktopTasks = preview("paper-ochre", "dark", "desktop", "tasks");
export const MossLightPhoneTasks = preview("paper-moss", "light", "phone", "tasks");
export const MossDarkPhoneTasks = preview("paper-moss", "dark", "phone", "tasks");
export const IndigoLightPhoneTasks = preview("paper-indigo", "light", "phone", "tasks");
export const IndigoDarkPhoneTasks = preview("paper-indigo", "dark", "phone", "tasks");
export const OchreLightPhoneTasks = preview("paper-ochre", "light", "phone", "tasks");
export const OchreDarkPhoneTasks = preview("paper-ochre", "dark", "phone", "tasks");
export const MossLightDesktopSignIn = preview("paper-moss", "light", "desktop", "sign-in");
export const MossDarkDesktopSignIn = preview("paper-moss", "dark", "desktop", "sign-in");
export const IndigoLightDesktopSignIn = preview("paper-indigo", "light", "desktop", "sign-in");
export const IndigoDarkDesktopSignIn = preview("paper-indigo", "dark", "desktop", "sign-in");
export const OchreLightDesktopSignIn = preview("paper-ochre", "light", "desktop", "sign-in");
export const OchreDarkDesktopSignIn = preview("paper-ochre", "dark", "desktop", "sign-in");
export const MossLightPhoneSignIn = preview("paper-moss", "light", "phone", "sign-in");
export const MossDarkPhoneSignIn = preview("paper-moss", "dark", "phone", "sign-in");
export const IndigoLightPhoneSignIn = preview("paper-indigo", "light", "phone", "sign-in");
export const IndigoDarkPhoneSignIn = preview("paper-indigo", "dark", "phone", "sign-in");
export const OchreLightPhoneSignIn = preview("paper-ochre", "light", "phone", "sign-in");
export const OchreDarkPhoneSignIn = preview("paper-ochre", "dark", "phone", "sign-in");
export const MossLightDesktopMembers = preview("paper-moss", "light", "desktop", "members");
export const MossDarkDesktopMembers = preview("paper-moss", "dark", "desktop", "members");
export const IndigoLightDesktopMembers = preview("paper-indigo", "light", "desktop", "members");
export const IndigoDarkDesktopMembers = preview("paper-indigo", "dark", "desktop", "members");
export const OchreLightDesktopMembers = preview("paper-ochre", "light", "desktop", "members");
export const OchreDarkDesktopMembers = preview("paper-ochre", "dark", "desktop", "members");
export const MossLightPhoneMembers = preview("paper-moss", "light", "phone", "members");
export const MossDarkPhoneMembers = preview("paper-moss", "dark", "phone", "members");
export const IndigoLightPhoneMembers = preview("paper-indigo", "light", "phone", "members");
export const IndigoDarkPhoneMembers = preview("paper-indigo", "dark", "phone", "members");
export const OchreLightPhoneMembers = preview("paper-ochre", "light", "phone", "members");
export const OchreDarkPhoneMembers = preview("paper-ochre", "dark", "phone", "members");
