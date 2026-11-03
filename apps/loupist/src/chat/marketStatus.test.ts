import { followedMarkets, marketFor, marketState } from "./marketStatus";

const utc = (iso: string) => Date.parse(iso);

describe("market status", () => {
  it("reads open, opening soon and closed from each exchange's own clock", () => {
    // Wednesday 10:00 in Seoul, 21:00 Tuesday in New York.
    expect(marketState("KRX", utc("2026-10-28T01:00:00Z"))).toEqual({ market: "KRX", state: "open", closesInMinutes: 330 });
    // Wednesday 09:00 in New York (EDT): the opening bell is half an hour away.
    expect(marketState("US", utc("2026-10-28T13:00:00Z"))).toEqual({ market: "US", state: "opensSoon", opensInMinutes: 30 });
    // Friday 17:00 in New York: Monday is too far away to count down, and so is tomorrow morning.
    expect(marketState("US", utc("2026-10-30T21:00:00Z"))).toEqual({ market: "US", state: "closed" });
    expect(marketState("KRX", utc("2026-10-31T03:00:00Z"))).toEqual({ market: "KRX", state: "closed" });
  });

  it("treats a lunch break as a pause and crypto as always trading", () => {
    // 12:00 in Tokyo: the afternoon session starts at 12:30.
    expect(marketState("TSE", utc("2026-10-28T03:00:00Z"))).toEqual({ market: "TSE", state: "opensSoon", opensInMinutes: 30 });
    expect(marketState("CRYPTO", utc("2026-10-31T03:00:00Z"))).toEqual({ market: "CRYPTO", state: "always" });
    expect(marketState("MOON", utc("2026-10-28T03:00:00Z"))).toBeUndefined();
  });

  it("closes on the exchanges' published holidays and cuts early-close days short", () => {
    // Thanksgiving (Thu Nov 26, 2026) at 10:00 in New York, and Hangeul Day (Fri Oct 9, 2026) at 10:00 in Seoul.
    expect(marketState("US", utc("2026-11-26T15:00:00Z"))).toEqual({ market: "US", state: "closed", holiday: true });
    expect(marketState("KRX", utc("2026-10-09T01:00:00Z"))).toEqual({ market: "KRX", state: "closed", holiday: true });
    // The day after Thanksgiving closes at 13:00 New York time: open at noon with an hour left, closed at 13:30.
    expect(marketState("US", utc("2026-11-27T17:00:00Z"))).toEqual({ market: "US", state: "open", closesInMinutes: 60, earlyCloseAtMs: utc("2026-11-27T18:00:00Z") });
    expect(marketState("US", utc("2026-11-27T18:30:00Z"))).toEqual({ market: "US", state: "closed" });
    // The countdown skips a holiday: 07:00 on the Tuesday after Seoul's Monday holiday (Oct 5) opens in two hours.
    expect(marketState("KRX", utc("2026-10-05T22:00:00Z"))).toEqual({ market: "KRX", state: "opensSoon", opensInMinutes: 120 });
    // 07:00 on the morning of Thanksgiving is a holiday, not "opens in 2h 30m".
    expect(marketState("US", utc("2026-11-26T12:00:00Z"))).toEqual({ market: "US", state: "closed", holiday: true });
    // An ordinary day is unchanged.
    expect(marketState("US", utc("2026-11-30T15:00:00Z"))).toEqual({ market: "US", state: "open", closesInMinutes: 360 });
  });

  it("never treats a date outside the published years as a holiday", () => {
    // Seollal 2027 (Feb 8) is not in KRX's published calendar yet, so Seoul reads as a regular Monday.
    expect(marketState("KRX", utc("2027-02-08T01:00:00Z"))).toEqual({ market: "KRX", state: "open", closesInMinutes: 330 });
    // Martin Luther King, Jr. Day 2028 is past NYSE's published years.
    expect(marketState("US", utc("2028-01-17T15:00:00Z"))).toEqual({ market: "US", state: "open", closesInMinutes: 360 });
    // Markets without a calendar keep regular hours (Tokyo on Culture Day 2026).
    expect(marketState("TSE", utc("2026-11-03T01:00:00Z"))).toEqual({ market: "TSE", state: "open", closesInMinutes: 90 });
  });

  it("maps typed market codes and orders followed markets by how many assets trade there", () => {
    expect(marketFor("kosdaq")).toBe("KRX");
    expect(marketFor("UNKNOWN")).toBeUndefined();
    expect(followedMarkets([
      { market: "NASDAQ" }, { market: "KRX" }, { market: "NYSE" }, { market: "UPBIT" }, { market: "UNKNOWN" },
    ])).toEqual(["US", "CRYPTO", "KRX"]);
  });
});
