// Holiday coverage of `market-calendar.json`. A year an exchange has not
// published is treated as a regular year (every weekday trades), so a missing
// current year silently mislabels holidays as sessions. The year in progress
// must be listed; from December the next year is reported until the exchange
// publishes it (KRX typically publishes in late December).
import { readFileSync, realpathSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The month (1-12) from which a missing next year is reported. */
export const NEXT_YEAR_NOTICE_MONTH = 12;

const DATE = /^(\d{4})-(\d{2})-(\d{2})$/;

export function parseAsOf(text) {
  const match = DATE.exec(text ?? "");
  const date = match && new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
  if (!date || date.toISOString().slice(0, 10) !== text) throw new Error(`--as-of must be YYYY-MM-DD, got ${text}`);
  return { year: date.getUTCFullYear(), month: date.getUTCMonth() + 1 };
}

/** Errors for an uncovered current year; notices (or errors) for the next one. */
export function calendarCoverage(table, asOf, { requireNextYear = false } = {}) {
  const errors = [];
  const notices = [];
  for (const [id, calendar] of Object.entries(table.markets ?? {})) {
    const years = new Set(calendar.years ?? []);
    if (!years.has(asOf.year)) {
      errors.push(`${id} does not list ${asOf.year}: every weekday would count as a trading day. Add the holidays ${calendar.name ?? id} published (${(calendar.sources ?? []).join(", ")}).`);
    }
    const next = asOf.year + 1;
    if (!years.has(next) && (requireNextYear || asOf.month >= NEXT_YEAR_NOTICE_MONTH)) {
      (requireNextYear ? errors : notices).push(`${id} does not list ${next} yet; add it once ${calendar.name ?? id} publishes it.`);
    }
  }
  return { errors, notices };
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const usage = "Usage: check-market-calendar.mjs [--as-of YYYY-MM-DD] [--require-next-year]";
  let asOfText = new Date().toISOString().slice(0, 10);
  let requireNextYear = false;
  try {
    for (let index = 0; index < args.length; index += 1) {
      if (args[index] === "--as-of" && index + 1 < args.length) asOfText = args[++index];
      else if (args[index] === "--require-next-year") requireNextYear = true;
      else throw new Error(usage);
    }
    const path = resolve(dirname(fileURLToPath(import.meta.url)), "../market-calendar.json");
    const { errors, notices } = calendarCoverage(JSON.parse(readFileSync(path, "utf8")), parseAsOf(asOfText), { requireNextYear });
    for (const line of notices) console.log(`notice: ${line}`);
    for (const line of errors) console.error(`error: ${line}`);
    console.log(`Market calendar as of ${asOfText}: ${errors.length} error(s), ${notices.length} notice(s)`);
    if (errors.length) process.exitCode = 1;
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
