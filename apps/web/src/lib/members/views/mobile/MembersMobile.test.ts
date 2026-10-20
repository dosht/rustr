import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { membersScreen as members } from "../members.fixtures";
import MembersMobile from "./MembersMobile.svelte";

describe("MembersMobile", () => {
  it("gives owners a card per member with a role picker and remove", async () => {
    const onIntent = vi.fn();
    render(MembersMobile, { view: members("owner-members"), onIntent });
    const bob = screen.getByText("bob@example.com").closest("li")!;
    await userEvent.selectOptions(within(bob).getByLabelText("Role of bob@example.com"), "viewer");
    expect(onIntent).toHaveBeenCalledWith({ type: "RoleChanged", email: "bob@example.com", role: "viewer" });
    await userEvent.click(within(bob).getByRole("button", { name: "Remove" }));
    expect(onIntent).toHaveBeenCalledWith({ type: "RemoveRequested", email: "bob@example.com" });
  });

  it("is read only for non-owners", () => {
    render(MembersMobile, { view: members("viewer-members"), onIntent: vi.fn() });
    expect(screen.queryByRole("button", { name: "Add member" })).not.toBeInTheDocument();
    expect(screen.getByText("You")).toBeInTheDocument();
  });
});
