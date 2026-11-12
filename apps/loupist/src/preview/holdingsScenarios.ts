/**
 * Crypto holdings and asset-class template scenarios
 * (finance-crypto-and-templates.md §8). Every scenario reaches its screen
 * through the app's own controls: Following › Import holdings, or the New Loupist dialog.
 */
import i18n from "../i18n";
import { prepareFixture, type PreviewBridge } from "./fixtures";
import type { PreviewScenario } from "./scenarios";
import { navigate } from "./scenarios/kit";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
};

const BTC_WALLET = "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3";
const EVM_WALLET = "0x71C7656EC7ab88b098defB751B7401B5f6d8976F";

/** Stocks from a CSV, an exchange file and two wallets, as a returning user has them. */
async function holdings(bridge: PreviewBridge) {
  await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
  const stocks = await bridge.holdingsImportCsv({ path: "/mock/holdings.csv" });
  await bridge.holdingsCommitCsv({ previewId: stocks.previewId });
  const crypto = await bridge.holdingsImportCryptoFile({ path: "/mock/kraken-ledger.csv" });
  await bridge.holdingsCommitCsv({ previewId: crypto.previewId });
  await bridge.walletAdd({ address: BTC_WALLET, name: "Cold storage" });
  await bridge.walletAdd({ address: EVM_WALLET });
}

/** Sets a field's value the way typing does, so React sees the change. */
function type(field: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, value);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

export function holdingsScenarios({ base, select, until, click }: Helpers): PreviewScenario[] {
  /** Following, reached from Schedule's Watching row until the sidebar row lands. */
  const openAbout = async (root: HTMLElement) => {
    await navigate(root, "schedule");
    await click('[data-schedule-task="watching"] .schedule-task-summary');
    await until(() => select("[data-testid=following-page] [data-holdings-kinds]"), "Following did not load");
  };
  const openSheet = async (root: HTMLElement) => {
    await openAbout(root);
    await click(".memory-holdings-update");
    await until(() => select("[data-holdings-sheet] .holdings-source"), "Holdings sheet did not open");
  };
  const sheet = (id: string, title: string, landmark: string, extra: { setup?: (bridge: PreviewBridge) => Promise<void> | void; enter?: (root: HTMLElement) => Promise<void> } = {}): PreviewScenario => ({
    ...base(id, title, () => select(landmark), async root => { await openSheet(root); await extra.enter?.(root); }),
    setup: async bridge => { await prepareFixture(bridge); await holdings(bridge); await extra.setup?.(bridge); },
  });
  const importCrypto = async () => {
    await until(() => select("[data-holdings-crypto-import]"), "Exchange file import missing");
    await click("[data-holdings-crypto-import]");
  };
  const addForm = async () => {
    await click(".holdings-add");
    return await until(() => select("[data-wallet-form] input") as HTMLInputElement | null, "Add a wallet did not open");
  };
  /** Create another Loupist from the header after first-run setup. */
  const templates = async (root: HTMLElement) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    await click(".loupist-header-new");
    await until(() => select(".loupist-template-grid"), "Templates did not appear");
  };
  return [
    sheet("holdings-sheet", "Your holdings: stocks, an exchange file and two wallets", "[data-holdings-sheet] .holdings-wallet"),
    sheet("holdings-crypto-preview", "Exchange file preview: Kraken codes merged, cash skipped", '[data-holdings-preview="crypto"] table', { enter: importCrypto }),
    sheet("holdings-crypto-mapping", "Exchange file with unknown columns: choose asset and balance", '[data-holdings-preview="mapping"] select', {
      setup: bridge => { bridge.pickFile = async () => "/mock/my-columns.csv"; },
      enter: importCrypto,
    }),
    sheet("holdings-trades-refused", "A file of trades is refused", '[data-holdings-preview="trades"] [role="alert"]', {
      setup: bridge => { bridge.pickFile = async () => "/mock/trades.csv"; },
      enter: importCrypto,
    }),
    sheet("holdings-wallet-add", "Add a wallet: the address is recognized", '[data-wallet-form] [data-wallet-detected="evm"]', {
      setup: bridge => { const list = bridge.walletsList.bind(bridge); bridge.walletsList = async () => (await list()).slice(0, 1); },
      enter: async () => { type(await addForm(), "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"); },
    }),
    sheet("holdings-wallet-secret-refused", "Add a wallet: a recovery phrase is cleared", "[data-wallet-form] [data-wallet-secret]", {
      enter: async () => { type(await addForm(), "legal winner thank year wave sausage worth useful legal winner thank yellow"); },
    }),
    sheet("holdings-wallet-read-failed", "A wallet read failed; the last good read stays", ".holdings-wallet .holdings-failed", {
      setup: bridge => {
        const list = bridge.walletsList.bind(bridge);
        bridge.walletsList = async () => (await list()).map((wallet, index) => index === 1
          ? { ...wallet, status: "failed" as const, lastReadAtMs: Date.now(), lastGoodReadAtMs: Date.now() - 26 * 3_600_000 }
          : wallet);
      },
    }),
    {
      ...base("memory-about-holdings-by-kind", "Following: holdings by kind, counts and dates only", () => select("[data-holdings-kinds]"), openAbout),
      setup: async bridge => { await prepareFixture(bridge); await holdings(bridge); },
    },
    {
      ...base("following-page", "Following: the list, Watch all and what this Loupist watches", () => select("[data-testid=following-page] [data-watch-all]") && select("[data-testid=following-page] .watching-reader"), openAbout),
      setup: async bridge => { await prepareFixture(bridge); await holdings(bridge); },
    },
    {
      ...base("onboarding-templates-asset-classes", "New Loupist: nine templates with the asset classes", () => {
        const grid = select('.loupist-template-grid');
        return grid && grid.querySelectorAll(".loupist-template-card").length === 9 ? grid : null;
      }, templates),
      setup: bridge => prepareFixture(bridge),
    },
    {
      ...base("onboarding-follow-starters-macro", "New Loupist: Macro & rates fills the research focus", () => {
        const fields = select(".loupist-template-fields");
        const focus = fields?.querySelector<HTMLTextAreaElement>("textarea");
        return focus?.value === i18n.t("onboarding:wizard.templates.macro.focus") ? fields : null;
      }, async root => {
        await templates(root);
        const macro = await until(() => [...document.querySelectorAll<HTMLButtonElement>('.loupist-template-card')]
          .find(card => card.querySelector("strong")?.textContent === i18n.t("onboarding:wizard.templates.macro.title")), "Macro template missing");
        macro.click();
        await until(() => macro.getAttribute("aria-pressed") === "true", "Macro template did not select");
        (select(".loupist-template-fields textarea") as HTMLElement | null)?.scrollIntoView?.({ block: "center" });
      }),
      setup: bridge => prepareFixture(bridge),
    },
  ];
}
