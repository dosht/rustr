import type { Meta, StoryObj } from "@storybook/svelte";
import { expect, fn, userEvent, within } from "storybook/test";
import { fixture } from "$lib/core/fixtures";
import AppStory from "./AppStory.svelte";

const meta = {
  title: "Pages/App/Root",
  component: AppStory,
  args: { onintent: fn() },
} satisfies Meta<typeof AppStory>;
export default meta;
type Story = StoryObj<typeof meta>;

export const PhoneLandscape: Story = {
  args: { vm: fixture("AppVm", "phone_job") },
  parameters: {
    viewport: { options: { phoneLandscape: { name: "Phone landscape", styles: { width: "839px", height: "412px" }, type: "mobile" } } },
  },
  globals: { viewport: { value: "phoneLandscape", isRotated: false } },
  beforeEach: () => {
    // Storybook's iframe resizing does not rotate the physical screen. Supply
    // that browser signal explicitly; the production landscape watcher still runs.
    const keys = ["width", "height", "orientation"] as const;
    const descriptors = keys.map((key) => Object.getOwnPropertyDescriptor(window.screen, key));
    Object.defineProperties(window.screen, {
      width: { configurable: true, value: 839 },
      height: { configurable: true, value: 412 },
      orientation: { configurable: true, value: Object.assign(new EventTarget(), { type: "landscape-primary" }) },
    });
    const matchMedia = window.matchMedia;
    window.matchMedia = (query) => {
      const result = matchMedia.call(window, query);
      if (query === "(pointer: fine)") Object.defineProperty(result, "matches", { value: false, configurable: true });
      return result;
    };
    return () => {
      keys.forEach((key, index) => {
        const descriptor = descriptors[index];
        if (descriptor) Object.defineProperty(window.screen, key, descriptor);
        else Reflect.deleteProperty(window.screen, key);
      });
      window.matchMedia = matchMedia;
    };
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const notice = await canvas.findByRole("dialog", { name: "Please rotate your device" });
    await expect(notice).toBeVisible();
    const background = canvasElement.querySelector("[inert]");
    await expect(background?.querySelector('[data-layout="phone"]')).not.toBeNull();
    await expect(background?.contains(notice)).toBe(false);
    await userEvent.keyboard("{Escape}");
    await expect(notice).toHaveAttribute("open");
  },
};
