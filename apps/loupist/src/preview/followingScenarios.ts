/**
 * Following and the New Loupist template scenarios. Every scenario reaches its
 * screen through the app's own controls: the Following sidebar row and its search
 * field, or the New Loupist dialog. Listing searches go to the mock host's OpenFIGI
 * (`bridge.assets`), whose listings are partly fictional; nothing is sent while typing.
 */
import i18n from "../i18n";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import type { PreviewScenario } from "./scenarios";
import { navigate, newLoupist } from "./scenarios/kit";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
};

type Follow = { name: string; ticker?: string; exchange?: string; figi?: string; securityType?: string };
const PAGE = "[data-testid=following-page]";
const FEW: Follow[] = [
  { name: "NVIDIA CORP", ticker: "NVDA", exchange: "US", figi: "BBG000BBJQV0", securityType: "Common Stock" },
  { name: "Hanul Precision", ticker: "HNP", exchange: "KS" },
  // Followed by the name the user typed, with no listing.
  { name: "Seongsu-dong office building stake" },
];
const LONG: Follow[] = [
  { name: "Northern Lights Hydroelectric Infrastructure and Transmission Holdings Corporation", ticker: "NLHT", exchange: "US" },
  // A listing name as OpenFIGI spells it, in capitals.
  { name: "ALPINE RESEARCH INTERNATIONAL HOLDINGS PLC AMERICAN DEPOSITARY SHARES EACH REPRESENTING ONE ORDINARY SHARE", ticker: "ALPH", exchange: "US", securityType: "ADR" },
  { name: "한울정밀기계공업 제2우선주 신형 전환상환우선주", ticker: "123456", exchange: "KQ" },
  { name: "Northern Lights Hydroelectric Co-Investment Partnership, Class B limited partner interests" },
];
const FIRST = ["Amber", "Basalt", "Cedar", "Delta", "Ember", "Fjord", "Granite", "Harbor", "Iris", "Juniper"];
const SECOND = ["Analytics", "Biosciences", "Capital", "Devices", "Energy", "Foods", "Gaming", "Holdings", "Industries", "Logistics"];
const MANY: Follow[] = Array.from({ length: 100 }, (_, index) => ({
  name: `${FIRST[index % 10]} ${SECOND[Math.floor(index / 10)]}`,
  ticker: `${FIRST[index % 10]!.slice(0, 2)}${SECOND[Math.floor(index / 10)]!.slice(0, 2)}`.toUpperCase(),
  exchange: ["US", "UN", "KS", "LN"][index % 4]!,
}));

/** Exactly these follows. */
async function followOnly(bridge: PreviewBridge, follows: readonly Follow[]) {
  await prepareFixture(bridge);
  for (const follow of follows) bridge.assets.installAsset(follow);
}

export function followingScenarios({ base, select, until }: Helpers): PreviewScenario[] {
  /** Following, from its sidebar row; the field is there once the page has rendered and the list is known. */
  const openFollowing = async (root: HTMLElement) => {
    await navigate(root, "following");
    await until(() => select(`${PAGE} .following-input`) && !select(`${PAGE} [data-following-loading]`), "Following did not load");
  };
  /** Types into the search field as a person would, then leaves focus there; `submit` presses Enter. */
  const type = async (root: HTMLElement, text: string, submit: boolean) => {
    await openFollowing(root);
    const input = select(`${PAGE} .following-input`) as HTMLInputElement;
    input.focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    if (submit) {
      await until(() => select(`${PAGE} [role=option][data-kind=search]`), "Search listings did not appear");
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
  };
  /** Clicks the result whose name is `name`. */
  const pick = async (name: string) => {
    const option = await until(() => [...document.querySelectorAll<HTMLElement>(`${PAGE} [role=option]`)].find(item => item.querySelector(".following-name")?.textContent === name), `${name} was not found`);
    option.click();
  };
  const lookup = (state: string) => () => select(`${PAGE} [data-lookup="${state}"]`);

  type Case = { id: string; title: string; landmark: () => Element | null | false; follows?: readonly Follow[]; query?: string | (() => string); submit?: boolean; ready?: (bridge: PreviewBridge) => void; enter?: (root: HTMLElement) => Promise<void> };
  const scenario = ({ id, title, landmark, follows = FEW, query, submit = true, ready, enter }: Case): PreviewScenario => ({
    ...base(id, title, () => landmark() || null, async root => {
      if (query !== undefined) await type(root, typeof query === "function" ? query() : query, submit);
      else await openFollowing(root);
      await enter?.(root);
    }),
    setup: async bridge => { await followOnly(bridge, follows); ready?.(bridge); },
  });
  /** Create another Loupist from the header after first-run setup. */
  const templates = async (root: HTMLElement) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    await newLoupist();
    await until(() => select(".loupist-template-grid"), "Templates did not appear");
  };
  const rows = (count: number) => () => document.querySelectorAll(`${PAGE} .following-row`).length === count ? select(`${PAGE} .following-list`) : null;
  return [
    scenario({ id: "following-empty", title: "Following with nothing followed: the field and one sentence", follows: [], landmark: () => select(`${PAGE} .following-empty`) }),
    scenario({ id: "following-few", title: "Following three assets: name and listing, one followed by name alone, and Stop following", landmark: rows(3) }),
    scenario({
      id: "following-many", title: "Following 100 assets, scrolled, with the field pinned under the header", follows: MANY,
      landmark: () => document.querySelectorAll(`${PAGE} .following-row`).length === 100 ? select(`${PAGE} .following-search`) : null,
      enter: async () => {
        const route = await until(() => document.querySelector<HTMLElement>(".following-route"), "Following route missing");
        route.scrollTop = 1400;
      },
    }),
    scenario({ id: "following-long-names", title: "Following: long names, one in a listing's capitals, end in an ellipsis", follows: LONG, landmark: rows(4) }),
    scenario({
      id: "following-typed", title: "Typing: followed matches at once, Search listings and Follow as typed; nothing is sent", query: "nvid", submit: false,
      landmark: () => select(`${PAGE} [role=option][data-kind=followed]`) && select(`${PAGE} [role=option][data-kind=typed]`) && select(`${PAGE} [role=option][data-kind=search]`),
    }),
    scenario({
      id: "following-results", title: "Search on Enter: the followed listing once, another OpenFIGI listing, and Follow as typed", query: "nvidia",
      landmark: () => lookup("done")() && select(`${PAGE} [role=option][data-kind=followed]`) && document.querySelectorAll(`${PAGE} [role=option][data-kind=candidate]`).length === 1 ? select(`${PAGE} .following-results`) : null,
    }),
    scenario({
      id: "following-searching", title: "Search waiting on OpenFIGI: followed matches and the searching line", query: "nvid",
      ready: bridge => { bridge.assets.lookupHold = true; }, landmark: lookup("waiting"),
    }),
    scenario({ id: "following-no-match", title: "Search: no listing matched; follow as typed, or ask Bot", follows: [], query: () => previewKo() ? "한빛중공업" : "Hanbit Heavy Industries", landmark: lookup("none") }),
    scenario({
      id: "following-lookup-off", title: "Search with Asset lookup off: one line to turn it on", query: "nvidia",
      ready: bridge => { bridge.assets.lookupNeedsConnection = true; }, landmark: lookup("connection"),
    }),
    scenario({ id: "following-busy", title: "Search while OpenFIGI asks to wait: one line and Try again", query: "nvidia", ready: bridge => { bridge.assets.lookupRateLimited = true; }, landmark: lookup("busy") }),
    scenario({ id: "following-failed", title: "Search failed: followed matches, Try again and Follow as typed", query: "nvid", ready: bridge => { bridge.assets.lookupFails = true; }, landmark: lookup("failed") }),
    scenario({
      id: "following-offline", title: "Search offline: followed matches and one line; nothing is sent", query: "nvid",
      ready: () => { Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false }); }, landmark: lookup("offline"),
    }),
    scenario({
      id: "following-adding", title: "Search: the picked listing is being followed", follows: [], query: "apple",
      ready: bridge => { bridge.assetFollowName = () => new Promise(() => {}); },
      enter: async () => { await until(lookup("done"), "Listings did not arrive"); await pick("APPLE INC"); }, landmark: () => select(`${PAGE} [role=option][data-pick=adding]`),
    }),
    scenario({
      id: "following-pick-failed", title: "Search: the pick could not be followed; the query and results stay with Try again", follows: [], query: "apple",
      ready: bridge => { bridge.assets.followFails = true; },
      enter: async () => { await until(lookup("done"), "Listings did not arrive"); await pick("APPLE INC"); }, landmark: () => select(`${PAGE} [role=option][data-pick=failed]`),
    }),
    {
      ...base("onboarding-templates-asset-classes", "New Loupist: eight templates with the asset classes", () => {
        const grid = select('.loupist-template-grid');
        return grid && grid.querySelectorAll(".loupist-template-card").length === 8 ? grid : null;
      }, templates),
      setup: bridge => prepareFixture(bridge),
    },
    {
      ...base("onboarding-follow-starters-macro", "New Loupist: Macro & rates offers starters to follow, one already followed", () => {
        const fields = select(".loupist-template-fields");
        const focus = fields?.querySelector<HTMLTextAreaElement>("textarea");
        return focus?.value === i18n.t("onboarding:wizard.templates.macro.focus") && fields?.querySelector('[data-template-starters="macro"] .loupist-starter[aria-pressed="true"]') ? fields : null;
      }, async root => {
        await templates(root);
        const macro = await until(() => [...document.querySelectorAll<HTMLButtonElement>('.loupist-template-card')]
          .find(card => card.querySelector("strong")?.textContent === i18n.t("onboarding:wizard.templates.macro.title")), "Macro template missing");
        macro.click();
        await until(() => macro.getAttribute("aria-pressed") === "true", "Macro template did not select");
        const starter = await until(() => select('[data-template-starters="macro"] .loupist-starter'), "Starters did not appear") as HTMLButtonElement;
        starter.click();
        await until(() => starter.getAttribute("aria-pressed") === "true", "Starter was not followed");
        starter.scrollIntoView?.({ block: "center" });
      }),
      setup: bridge => prepareFixture(bridge),
    },
  ];
}
