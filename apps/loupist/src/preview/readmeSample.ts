import type { PreviewBridge } from "./fixtures";

const METHOD = "Separate durable AI-chip demand from a temporary packaging bottleneck";
const PRIOR_METHOD = "Check HBM allocation across an AI-capacity ramp";
const RATIONALE = "A strong shipment quarter did not show that high-bandwidth memory and advanced packaging had loosened.";
const THEN = "Compare data-center GPU shipments with HBM supply. Treat a strong quarter as demand.";
const NOW = "Compare GPU shipments, HBM allocation, and advanced-packaging slots. Confirm the constraint with lead times, and test whether the limit is memory, packaging, or the leading-edge wafer.";

const CHECKS = [
  "HBM allocation loosens before the next packaging slot",
  "Advanced-packaging lead times stop rising",
  "Data-center GPU shipments stay ahead of HBM supply",
];

/** Home briefing for the public clips. The shared fixture is a different industry. */
export function applyReadmeBriefing(bridge: PreviewBridge) {
  const list = bridge.inboxList.bind(bridge);
  bridge.inboxList = async request => {
    const page = await list(request);
    for (const item of page.items) {
      if (item.runId !== "preview-briefing-1") continue;
      item.summary = "High-bandwidth memory and TSMC packaging slots are the constraint in this AI accelerator sample. The chip design is not the binding step.";
    }
    return page;
  };
  const journals = bridge.journalList.bind(bridge);
  bridge.journalList = async request => {
    const entries = await journals(request);
    entries.forEach((entry, index) => {
      const claim = CHECKS[index] ?? CHECKS[0]!;
      entry.judgment = claim;
      if (entry.check) { entry.check.claim = claim; entry.check.metric = claim; }
    });
    return entries;
  };
}

/** Chat answer for the public page. Figures stay inside a labeled sample note. */
export function applyReadmeChat(bridge: PreviewBridge) {
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await select(loupistId);
    const thread = workspace.threads[0];
    if (!thread) return workspace;
    thread.title = "Where AI accelerator supply is tight";
    const question = thread.messages.find(message => message.role === "user" && !message.steer);
    const answer = thread.messages.find(message => message.id === "preview-answer");
    if (question) question.content = "What is tight in the AI accelerator supply chain right now?";
    if (answer) {
      const evidence = answer.evidence_refs?.[0];
      answer.content = evidence
        ? `The binding constraint in this sample is high-bandwidth memory and advanced packaging, not the accelerator design. SK hynix and Samsung Electronics supply HBM. TSMC's CoWoS step is what turns those parts into a data-center GPU. ASML's EUV tools sit under TSMC's leading-edge wafers. [1](#evidence-${evidence.id})\n\nA strong shipment quarter does not show the bottleneck has moved. The next check is whether HBM allocation and packaging slots loosen together.`
        : answer.content;
      if (evidence) {
        evidence.title = "Sample note: HBM allocation and advanced-packaging slots";
        evidence.provider = "Sample research note";
        evidence.excerpt = "HBM supply and advanced-packaging slots, not the accelerator design, are the binding constraint in this note.";
      }
    }
    return workspace;
  };
}

/** Lens revision copy for the public page. The shared learning fixture is a different industry. */
export function applyReadmeLens(bridge: PreviewBridge) {
  const reviews = bridge.learningReviewList.bind(bridge);
  bridge.learningReviewList = async request => {
    const page = await reviews(request);
    const created = page.find(review => review.reviewId === "learning-created");
    if (created) created.rationale = RATIONALE;
    return page;
  };
  const events = bridge.knowledgeGrowthEvents.bind(bridge);
  bridge.knowledgeGrowthEvents = async request => {
    const page = await events(request);
    for (const event of page.events) {
      if (event.eventId !== "learning-revision") continue;
      event.beforeSummary = THEN;
      event.afterSummary = NOW;
      event.rationale = RATIONALE;
      event.summary = "Added packaging and HBM";
    }
    return page;
  };
  const read = bridge.knowledgeLensRead.bind(bridge);
  bridge.knowledgeLensRead = async request => {
    const page = await read(request);
    if (request.recordId === "map-lens-2") page.title = METHOD;
    if (request.recordId === "map-lens-1") page.title = PRIOR_METHOD;
    return page;
  };
}
