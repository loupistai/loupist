import { act, fireEvent, render, screen, within } from "@testing-library/react";
import i18n from "@/i18n";
import { AppDateField } from "./app-date-field";

afterEach(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });

it("names the month in the app language", async () => {
  const onChange = vi.fn();
  await act(async () => { await i18n.changeLanguage("en"); });
  const view = render(<AppDateField aria-label="Date" value="2026-03-03" onChange={onChange} />);
  expect(document.querySelector("input[type=date]")).toBeNull();
  expect(within(screen.getByRole("combobox", { name: "Month" })).getByRole("option", { selected: true })).toHaveTextContent("Mar");
  await act(async () => { await i18n.changeLanguage("ko"); });
  view.rerender(<AppDateField aria-label="날짜" value="2026-03-03" onChange={onChange} />);
  expect(within(screen.getByRole("combobox", { name: "월" })).getByRole("option", { selected: true })).toHaveTextContent("3월");
  await act(async () => { await i18n.changeLanguage("en"); });
  view.rerender(<AppDateField aria-label="Date" value="2999-01-01" max="2026-11-03" onChange={onChange} />);
  expect(within(screen.getByRole("combobox", { name: "Year" })).getByRole("option", { selected: true })).toHaveTextContent("2999");
});

it("keeps a year chosen before the month and emits the date once the day is set", () => {
  const onChange = vi.fn();
  render(<AppDateField aria-label="Date" value="" onChange={onChange} />);
  fireEvent.change(screen.getByRole("combobox", { name: "Year" }), { target: { value: "2026" } });
  expect(onChange).not.toHaveBeenCalled();
  expect(screen.getByRole("combobox", { name: "Year" })).toHaveValue("2026");
  fireEvent.change(screen.getByRole("combobox", { name: "Month" }), { target: { value: "3" } });
  fireEvent.change(screen.getByRole("combobox", { name: "Day" }), { target: { value: "3" } });
  expect(onChange).toHaveBeenCalledWith("2026-03-03");
});
