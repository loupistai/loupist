import { displayHost } from "./displayHost";

it.each([
  ["a page", "https://www.example.com/report?x=1", "example.com"],
  ["a host without www", "https://dart.fss.or.kr/x", "dart.fss.or.kr"],
  ["an address with credentials", "https://user:pw@example.com/", ""],
  ["a non-http address", "javascript:alert(1)", ""],
  ["no address", undefined, ""],
])("names the host of %s", (_name, url, host) => {
  expect(displayHost(url)).toBe(host);
});
