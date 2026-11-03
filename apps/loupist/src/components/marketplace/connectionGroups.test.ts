import i18n from "../../i18n";
import { connectionGroups } from "../../marketplace/groups";

afterEach(async () => { await i18n.changeLanguage("en"); });
it("translates every bundled group and keeps unknown connection names", async () => {
  const names = ["Market data", "News", "Filings", "Research", "Web Research", "World Bank", "Third-party"];
  const plugins = names.map(name => ({ name, interface: { displayName: name } }));
  const entries = names.map(plugin => ({ plugin }));
  await i18n.changeLanguage("ko");
  expect(connectionGroups(plugins, entries).map(group => group.label)).toEqual(["시장 데이터", "뉴스", "공시", "리서치", "웹 리서치", "세계은행", "Third-party"]);
  await i18n.changeLanguage("en");
  expect(connectionGroups(plugins, entries).map(group => group.label)).toEqual(names);
});
it("collects regional providers under a translated Korea heading", async () => {
  await i18n.changeLanguage("ko");
  expect(connectionGroups([{ name: "krx", category: "Korea", interface: { displayName: "KRX" } }], [{ plugin: "krx" }])).toEqual([{ id: "korea", label: "한국", entries: [{ plugin: "krx" }] }]);
});
