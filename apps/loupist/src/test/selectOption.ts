import { screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

export async function openSelect(user: UserEvent, trigger: HTMLElement) {
  await user.click(trigger);
  return within(await screen.findByRole("listbox"));
}

export async function chooseSelectOption(user: UserEvent, trigger: HTMLElement, name: string | RegExp) {
  const list = await openSelect(user, trigger);
  await user.click(list.getByRole("option", { name }));
}
