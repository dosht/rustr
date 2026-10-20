import { render, screen, within } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { membersScreen as members } from "../members.fixtures";
import MembersDesktop from "./MembersDesktop.svelte";

describe("MembersDesktop", () => {
  it("lets owners add, change and remove members", async () => {
    const onIntent = vi.fn();
    render(MembersDesktop, { view: members("owner-members"), onIntent, heading: "Members" });
    await userEvent.type(screen.getByLabelText("Member email"), "zoe@example.com");
    await userEvent.selectOptions(screen.getByLabelText("Role"), "viewer");
    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(onIntent).toHaveBeenCalledWith({ type: "AddSubmitted", email: "zoe@example.com", role: "viewer" });

    await userEvent.selectOptions(screen.getByLabelText("Role of bob@example.com"), "owner");
    expect(onIntent).toHaveBeenCalledWith({ type: "RoleChanged", email: "bob@example.com", role: "owner" });

    const eve = screen.getByText("eve@example.com").closest("li")!;
    await userEvent.click(within(eve).getByRole("button", { name: "Remove" }));
    expect(onIntent).toHaveBeenCalledWith({ type: "RemoveRequested", email: "eve@example.com" });
  });

  it("only shows roles to everyone else", () => {
    render(MembersDesktop, { view: members("viewer-members"), onIntent: vi.fn(), heading: "Members" });
    expect(screen.queryByLabelText("Member email")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
    expect(screen.getByText("(you)")).toBeInTheDocument();
  });
});
