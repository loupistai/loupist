import userEvent from "@testing-library/user-event";
import { chooseSelectOption } from "@/test/selectOption";
import { act, render, screen, waitFor } from "@testing-library/react";
import i18n from "@/i18n";
import { AppDateField } from "./app-date-field";

afterEach(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });

it("names the month in the app language", async () => {
  const onChange = vi.fn();
  await act(async () => { await i18n.changeLanguage("en"); });
  const view = render(<AppDateField aria-label="Date" value="2026-03-03" onChange={onChange} />);
  expect(document.querySelector("input[type=date]")).toBeNull();
  expect(screen.getByRole("combobox", { name: "Month" })).toHaveTextContent("Mar");
  await act(async () => { await i18n.changeLanguage("ko"); });
  view.rerender(<AppDateField aria-label="날짜" value="2026-03-03" onChange={onChange} />);
  expect(screen.getByRole("combobox", { name: "월" })).toHaveTextContent("3월");
  await act(async () => { await i18n.changeLanguage("en"); });
  view.rerender(<AppDateField aria-label="Date" value="2999-01-01" max="2026-11-03" onChange={onChange} />);
  expect(screen.getByRole("combobox", { name: "Year" })).toHaveTextContent("2999");
});

it("keeps a year chosen before the month and emits the date once the day is set", async () => {
  const onChange = vi.fn();
  const user = userEvent.setup();
  render(<AppDateField aria-label="Date" value="" onChange={onChange} />);
  await chooseSelectOption(user, screen.getByRole("combobox", { name: "Year" }), "2026");
  expect(onChange).not.toHaveBeenCalled();
  expect(screen.getByRole("combobox", { name: "Year" })).toHaveTextContent("2026");
  await chooseSelectOption(user, screen.getByRole("combobox", { name: "Month" }), "Mar");
  await chooseSelectOption(user, screen.getByRole("combobox", { name: "Day" }), "3");
  expect(onChange).toHaveBeenCalledWith("2026-03-03");
});

it("keeps required date parts invalid until chosen and leaves disabled parts unfocusable", async () => {
  const onChange = vi.fn();
  const view = render(<form aria-label="Check"><AppDateField aria-label="Date" value="" required onChange={onChange} /></form>);
  const form = screen.getByRole("form", { name: "Check" });
  expect(form).toBeInvalid();
  const user = userEvent.setup();
  await chooseSelectOption(user, screen.getByRole("combobox", { name: "Year" }), "2026");
  expect(form).toBeInvalid();
  view.rerender(<form aria-label="Check"><AppDateField aria-label="Date" value="2026-03-03" required onChange={onChange} /></form>);
  await waitFor(() => expect(form).toBeValid());
  view.rerender(<form aria-label="Check"><AppDateField aria-label="Date" value="2026-03-03" required disabled onChange={onChange} /></form>);
  for (const control of screen.getAllByRole("combobox")) expect(control).toBeDisabled();
});
