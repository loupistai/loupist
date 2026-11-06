import type { LearnedDigest, LensLogEntryView, PageView, SectionView } from "../types";

/** The answer whose Learned line the `chat-learned` scenario opens. */
export const LEARNED_MESSAGE = "preview-answer";
const AT = Date.parse("2026-10-27T09:30:00Z");

const section = (key: string, heading: string, markdown: string, extra: Partial<SectionView> = {}): SectionView => ({
  key, heading, markdown, digest: `digest-${key}`, basis: { refs: [] }, chip: "none", origin: "loupist", contested: false, contestedClaimIds: [], updatedAtMs: AT, ...extra,
});

const page = (recordId: string, kind: PageView["kind"], title: string, summary: string, sections: SectionView[], extra: Partial<PageView> = {}): PageView => ({
  recordId, revisionId: `preview-${recordId}`, revisionDigest: "a".repeat(64), kind, title, archived: false, paused: false, learned: true, createdAtMs: AT, updatedAtMs: AT,
  body: kind === "idea" ? { kind, summary, sections, idea: { status: "open", registeredAtMs: AT, statusAtMs: AT } } : { kind: kind as "subject" | "method" | "perspective", summary, sections },
  suggestedSections: [], anchors: [], links: [], backlinks: [], history: [], ...extra,
});

/** Fictional pages in English or Korean: a topic with every kind of basis chip, an idea, a method and an outlook. */
export function previewPages(ko: boolean, methodTitle: string): PageView[] {
  const text = (en: string, kr: string) => ko ? kr : en;
  const subject = page("preview-subject", "subject", text("Hanul Precision", "한울정밀"),
    text("A precision-parts maker adding capacity while margins thin; the dip looks tied to qualification costs.", "생산능력을 늘리면서 이익률이 얇아진 정밀부품 제조사예요. 이익률 하락은 인증 비용과 관련 있어 보여요."), [
      section("overview", text("Overview", "개요"), text("Revenue grew 11% to ₩184.2bn while operating margin fell from 16.2% to 14.8%. Most of the drop is qualification cost at the new line.", "매출은 11% 늘어 1,842억 원이 됐고 영업이익률은 16.2%에서 14.8%로 내렸어요. 하락분 대부분은 새 라인의 인증 비용이에요."), {
        chip: "source", basis: { refs: [{ kind: "claim", claimId: "preview-claim-0" }, { kind: "span", recordId: "preview-sources", revisionDigest: "a".repeat(64), spanId: "preview-span" }] },
      }),
      section("value-chain", text("Value chain", "공급망"), text("Supplies [Morrow Components](world:preview-node-1) and buys materials from [Eunbit Materials](world:preview-node-4).", "[Morrow Components](world:preview-node-1)에 납품하고 [Eunbit Materials](world:preview-node-4)에서 소재를 사요."), {
        chip: "source", basis: { refs: [{ kind: "claim", claimId: "preview-claim-1" }] },
      }),
      section("goals", text("What you told me", "내가 말한 내용"), text("You hold it for the dividend, not for growth, so a thinner margin matters only if the payout is at risk.", "성장이 아니라 배당 때문에 들고 있어서, 이익률이 얇아져도 배당이 위험할 때만 중요해요."), {
        chip: "said", origin: "user", basis: { refs: [{ kind: "said", messageId: "preview-user" }] },
      }),
      section("views", text("Where it is heading", "앞으로의 방향"), text("If utilization passes 80% by spring, the margin should recover without new cost cuts. See [the capacity idea](lens:preview-idea).", "가동률이 봄까지 80%를 넘으면 추가 비용 절감 없이도 이익률이 회복될 거예요. [생산능력 아이디어](lens:preview-idea)를 보세요."), {
        chip: "view", basis: { refs: [{ kind: "page", recordId: "preview-idea" }], uncertainty: text("Two quarters of data; the supplier survey is the only outside check.", "두 분기 자료뿐이고, 공급사 설문이 유일한 외부 확인이에요.") },
      }),
      section("risks", text("Risks", "위험"), text("Customer concentration: the filing says the top buyer is 38% of revenue, a supplier survey says 45%.", "고객 집중도: 공시는 최대 고객이 매출의 38%라고 하고, 공급사 설문은 45%라고 해요."), {
        chip: "source", contested: true, contestedClaimIds: ["preview-claim-2"], basis: { refs: [{ kind: "claim", claimId: "preview-claim-2" }] },
      }),
    ], {
      anchors: [{ role: "subject", nodeId: "preview-node-0", name: text("Hanul Precision", "한울정밀") }, { role: "mention", nodeId: "preview-node-1", name: "Morrow Components" }, { role: "mention", nodeId: "preview-node-4", name: "Eunbit Materials" }],
      links: [{ recordId: "preview-idea", title: text("Margins recover once utilization passes 80%", "가동률이 80%를 넘으면 이익률이 회복돼요"), kind: "idea" }],
      suggestedSections: ["overview", "value-chain", "risks", "views"],
      history: [
        { opId: "op-subject-2", op: "put", outcome: "added", key: "risks", origin: "turn", chatId: "preview-chat", messageId: LEARNED_MESSAGE, createdAtMs: AT + 2_000 },
        { opId: "op-subject-1", op: "put", outcome: "added", key: "overview", origin: "turn", chatId: "preview-chat", messageId: LEARNED_MESSAGE, createdAtMs: AT + 1_000 },
      ],
    });
  const idea = page("preview-idea", "idea", text("Margins recover once utilization passes 80%", "가동률이 80%를 넘으면 이익률이 회복돼요"),
    text("An unverified link: fixed costs dilute as the new line fills, so the margin dip should be temporary.", "아직 확인되지 않은 연결이에요. 새 라인이 차면 고정비가 희석돼 이익률 하락은 일시적일 거예요."), [
      section("claim", text("Claim", "주장"), text("Operating margin returns to about 16% when utilization of the new line passes 80%.", "새 라인 가동률이 80%를 넘으면 영업이익률이 16% 안팎으로 돌아와요."), {
        chip: "view", basis: { refs: [{ kind: "page", recordId: "preview-subject", sectionKey: "overview" }], uncertainty: text("No quarter yet shows the line above 80%.", "가동률이 80%를 넘은 분기는 아직 없어요.") },
      }),
    ], {
      anchors: [{ role: "subject", nodeId: "preview-node-0", name: text("Hanul Precision", "한울정밀") }],
      history: [{ opId: "op-idea-1", op: "put", outcome: "added", key: "claim", origin: "turn", chatId: "preview-chat", messageId: LEARNED_MESSAGE, createdAtMs: AT + 3_000 }],
    });
  const method = page("preview-method", "method", methodTitle, text("Compare margins, utilization and cash conversion before interpreting a single quarter.", "한 분기를 해석하기 전에 이익률, 가동률, 현금 전환을 비교해요."), [
    section("use-when", text("When to use it", "쓰는 때"), text("A manufacturer adds capacity while reported margins decline.", "제조사가 생산능력을 늘리는 동안 보고된 이익률이 낮아질 때요.")),
    section("steps", text("Steps", "단계"), text("1. Compare revenue, utilization and inventory days.\n2. Reconcile one-time qualification costs.\n3. Check whether cash conversion supports the demand story.", "1. 매출, 가동률, 재고일수를 비교해요.\n2. 일회성 인증 비용을 맞춰 봐요.\n3. 현금 전환이 수요 이야기를 뒷받침하는지 확인해요.")),
  ]);
  const outlook = page("preview-perspective", "perspective", "Evidence before conviction", text("Treat explanations as claims to examine.", "설명을 검토할 주장으로 다뤄요."), [
    section("beliefs", text("Beliefs", "믿는 것"), text("Separate observation, inference and uncertainty.", "관찰, 추론, 불확실성을 나눠요.")),
  ]);
  return [subject, idea, method, outlook];
}

export const previewLearned = (pages: readonly PageView[]): LearnedDigest => ({
  messageId: LEARNED_MESSAGE,
  pages: pages.filter(item => item.history.some(entry => entry.messageId === LEARNED_MESSAGE)).map(item => ({
    recordId: item.recordId, title: item.title, kind: item.kind, created: true,
    outcomes: item.history.filter(entry => entry.messageId === LEARNED_MESSAGE).map(entry => ({ opId: entry.opId, outcome: entry.outcome, key: entry.key })),
  })),
});

export type PreviewLog = LensLogEntryView;
