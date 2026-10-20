import type { MembersView } from "../../core";
import { signedIn } from "../../core/fixtures";

export function membersScreen(name: string): MembersView {
  const view = signedIn(name);
  if (view.screen.kind !== "members") throw new Error(`${name} is not a members screen`);
  return view.screen.view;
}
