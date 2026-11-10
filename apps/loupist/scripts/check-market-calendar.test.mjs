import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { calendarCoverage, parseAsOf } from "./check-market-calendar.mjs";

const table = {
  markets: {
    KRX: { name: "Korea Exchange", years: [2026], sources: ["https://example.test/krx"] },
    US: { name: "NYSE", years: [2026, 2027], sources: ["https://example.test/nyse"] },
  },
};

test("the year in progress must be published", () => {
  const { errors } = calendarCoverage(table, parseAsOf("2027-01-04"));
  assert.equal(errors.length, 1);
  assert.match(errors[0], /^KRX does not list 2027: every weekday would count as a trading day/);
  assert.deepEqual(calendarCoverage(table, parseAsOf("2026-06-01")), { errors: [], notices: [] });
});

test("a missing next year is a notice from December, or an error on request", () => {
  assert.deepEqual(calendarCoverage(table, parseAsOf("2026-11-30")).notices, []);
  const december = calendarCoverage(table, parseAsOf("2026-12-01"));
  assert.deepEqual(december.errors, []);
  assert.deepEqual(december.notices, ["KRX does not list 2027 yet; add it once Korea Exchange publishes it."]);
  const strict = calendarCoverage(table, parseAsOf("2026-06-01"), { requireNextYear: true });
  assert.equal(strict.errors.length, 1);
  assert.deepEqual(strict.notices, []);
});

test("dates are strict calendar days", () => {
  assert.deepEqual(parseAsOf("2028-02-29"), { year: 2028, month: 2 });
  for (const text of ["2027-02-29", "2027-1-04", "today", undefined]) assert.throws(() => parseAsOf(text));
});

test("the bundled calendar covers the years its own test dates use", () => {
  const bundled = JSON.parse(readFileSync(new URL("../market-calendar.json", import.meta.url), "utf8"));
  assert.deepEqual(calendarCoverage(bundled, parseAsOf("2026-11-11")).errors, []);
});
