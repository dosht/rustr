import { render, screen } from "@testing-library/svelte";
import { beforeAll, expect, it } from "vitest";
import { fixture } from "$lib/core/fixtures";
import type { StatusBadge } from "$lib/core/types/StatusBadge";
import { i18n } from "$lib/i18n/index.svelte";
import TranscriptHeader from "./TranscriptHeader.svelte";

beforeAll(() => i18n.use("en"));

function header(badge: Partial<StatusBadge>) {
  const base = fixture("TranscriptVm", "completed");
  if (!base.status_badge) throw new Error("the completed fixture has a status badge");
  return { vm: { ...base, status_badge: { ...base.status_badge, ...badge } } };
}

it("draws the badge tone and glyph core projects, not a table of its own", () => {
  const { container, rerender } = render(TranscriptHeader, header({ tone: "danger", icon: "cross", spinning: false, status: "completed" }));
  const badge = screen.getByText(/./, { selector: "span.rounded-full" });
  expect(badge.className).toContain("text-error");
  expect(container.querySelector("svg")).not.toBeNull();
  rerender(header({ tone: "progress", icon: null, spinning: true, status: "failed" }));
  expect(screen.getByText(/./, { selector: "span.rounded-full" }).className).toContain("text-progress");
  expect(container.querySelector(".animate-spin")).not.toBeNull();
  expect(container.querySelector("svg")).toBeNull();
});
