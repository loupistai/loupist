import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ConfiguredModel, ModelProviderOption } from "../../types";
import { ModelSelectionMenu } from "./ModelSelectionMenu";

const providers: ModelProviderOption[] = [{
  id: "provider", label: "Provider", credential_label: "API key", description: "",
  api_key: true, oauth: null, credential_source: "saved", recommended: false,
  group: "more", console_url: null, sign_in_flow: null, billing_note: null,
  status: { state: "connected", credential_kind: "api_key", notice: null, last_limit_at_ms: null, connected_at_ms: null, used_by: { schedules: 0, chat_default: false, world_models: 0 } },
}];

const models: ConfiguredModel[] = ["General", "Research fast", "Research deep"].map((name) => ({
  id: name, name, model: name, provider: "provider", input: ["text"], reasoning: true,
  context_window: 1000, max_tokens: 100, thinking_levels: ["low", "medium"],
  thinking_level_map: { low: "low", medium: "medium" }, run_controls: [], credential_source: "saved",
}));

function renderMenu() {
  const onSelectionChange = vi.fn();
  render(<ModelSelectionMenu models={models} providers={providers}
    selection={{ model_profile_id: "General", thinking_level: "low", run_options: {} }}
    onSelectionChange={onSelectionChange} />);
  return onSelectionChange;
}

it("selects a filtered model from search with the keyboard", async () => {
  const [entryKey, firstResult, nextKey, selected] = ["ArrowDown", "Research fast", "ArrowDown", "Research deep"];
  const user = userEvent.setup();
  const onSelectionChange = renderMenu();
  const trigger = screen.getByRole("button", { name: /Model settings/ });

  await user.tab();
  expect(trigger).toHaveFocus();
  await user.keyboard("{Enter}");
  const search = screen.getByRole("searchbox", { name: "Search models" });
  expect(search).toHaveFocus();
  await user.keyboard("research");
  expect(screen.queryByRole("menuitemradio", { name: "General" })).toBeNull();

  await user.keyboard(`{${entryKey}}`);
  expect(screen.getByRole("menuitemradio", { name: firstResult })).toHaveFocus();
  await user.keyboard(`{${nextKey}}`);
  await waitFor(() => expect(screen.getByRole("menuitemradio", { name: selected })).toHaveFocus());
  await user.keyboard("{Enter}");
  expect(onSelectionChange).toHaveBeenCalledExactlyOnceWith({
    model_profile_id: selected, thinking_level: "medium", run_options: {},
  });
});


it("moves between search and model choices with Tab and restores the trigger on Escape", async () => {
  const user = userEvent.setup();
  renderMenu();
  const trigger = screen.getByRole("button", { name: /Model settings/ });
  await user.click(trigger);
  const search = screen.getByRole("searchbox", { name: "Search models" });
  expect(search).toHaveFocus();
  await user.tab();
  expect(screen.getByRole("menuitemradio", { name: "General" })).toHaveFocus();
  await user.tab({ shift: true });
  expect(search).toHaveFocus();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(trigger).toHaveFocus();
});
