// In-memory World schema store for the mock bridge and previews.

import type { ExtensionType, SchemaRemovalKind, WorldExtension, WorldSchemaView } from "../../contracts/knowledge";

const processDescription = "A named semiconductor manufacturing process, such as deposition, etch, lithography or packaging, kept distinct from the company that runs it and the product made on it.";
const capacityDescription = "A stated plan to add manufacturing capacity, including the site, the process it expands, and when the new volume is expected to start.";

function emptyWorldSchema(now = 0): WorldSchemaView {
  return { extension: { version: 0, types: [], relations: [], updatedAtMs: now }, proposal: null };
}

const SEMICONDUCTOR_FOCUS = ["Chips, memory, foundries and equipment", "칩, 메모리, 파운드리, 장비"];
const DIVIDEND_FOCUS = ["Dividend payers and payout safety", "배당 기업과 배당 지속성"];

/** Curated seed when the stored focus is still that template's default text. */
export function curatedSeedView(template: string | undefined, focus: string, now = 0): WorldSchemaView | null {
  const text = focus.trim();
  const extension = template === "semiconductors" && SEMICONDUCTOR_FOCUS.includes(text)
    ? semiconductorExtension(now)
    : template === "dividends" && DIVIDEND_FOCUS.includes(text)
      ? dividendExtension(now)
      : null;
  return extension ? { extension, proposal: null } : null;
}

function semiconductorExtension(now: number): WorldExtension {
  const processNode: ExtensionType = {
    name: "ProcessNode",
    description: "A named semiconductor manufacturing process generation, such as N3, 18A or 1b DRAM. Not the fab that runs it (Place) and not the chip made on it (Product).",
    category: "product",
    labels: { en: "Process node", ko: "공정 노드" },
    blurbs: { en: "Process generations such as N3 or 18A", ko: "N3, 18A 같은 반도체 공정 세대" },
    fields: [],
  };
  return {
    version: 1,
    updatedAtMs: now,
    types: [processNode],
    relations: [
      {
        name: "QUALIFIED_BY",
        description: "The target customer has qualified or certified the source product for use. Keep the date and stage in the fact.",
        signatures: [["Product", "Organization"]],
        labels: { en: "qualified by", ko: "인증" },
      },
      {
        name: "MADE_ON",
        description: "The source product is manufactured on the target process node.",
        signatures: [["Product", "ProcessNode"]],
        labels: { en: "made on", ko: "제조 공정" },
      },
    ],
  };
}

function dividendExtension(now: number): WorldExtension {
  return {
    version: 1,
    updatedAtMs: now,
    types: [{
      name: "CapitalReturnPolicy",
      description: "A company's stated policy for returning cash to shareholders: a payout target, a progressive dividend commitment or a buyback authorization. A single dividend declaration is an Event, and a paid amount is a Metric.",
      category: "event",
      labels: { en: "Capital return policy", ko: "주주환원 정책" },
      blurbs: { en: "Dividend and buyback policies", ko: "배당·자사주 매입 같은 주주환원 정책" },
      fields: [{ name: "payout_target", description: "The stated payout ratio or dividend target, as written." }],
    }],
    relations: [{
      name: "HAS_RETURN_POLICY",
      description: "The organization has adopted or announced the target capital return policy.",
      signatures: [["Organization", "CapitalReturnPolicy"]],
      labels: { en: "has return policy", ko: "환원 정책" },
    }],
  };
}

/** Semiconductors seed: ProcessNode, QUALIFIED_BY, MADE_ON, plus a pending CapacityPlan review. */
export function semiconductorTrackingView(now = 0): WorldSchemaView {
  const processNode: ExtensionType = {
    name: "ProcessNode",
    description: processDescription,
    category: "product",
    labels: { en: "Process node", ko: "공정 노드" },
    blurbs: { en: "Process generations such as N3 or 18A", ko: "N3, 18A 같은 반도체 공정 세대" },
    fields: [],
  };
  const capacityPlan: ExtensionType = {
    name: "CapacityPlan",
    description: capacityDescription,
    category: "event",
    labels: { en: "Capacity plan", ko: "증설 계획" },
    blurbs: { en: "Announced plans to add production capacity", ko: "생산능력을 늘리는 증설 계획" },
    fields: [],
  };
  return {
    extension: {
      version: 1,
      updatedAtMs: now,
      types: [processNode],
      relations: [
        {
          name: "QUALIFIED_BY",
          description: "Links a company or product to the process node it has qualified for production use.",
          signatures: [["Organization", "ProcessNode"], ["Product", "ProcessNode"]],
          labels: { en: "qualified by", ko: "인증" },
        },
        {
          name: "MADE_ON",
          description: "Links a product to the process node it was manufactured on.",
          signatures: [["Product", "ProcessNode"]],
          labels: { en: "made on", ko: "제조 공정" },
        },
      ],
    },
    proposal: {
      proposalId: "preview-capacity-plan",
      kind: "review",
      status: "pending",
      createdAtMs: now,
      types: [capacityPlan],
      relations: [
        {
          name: "EXPANDS_CAPACITY",
          description: "Links a capacity plan to the process node or site whose output it expands.",
          signatures: [["CapacityPlan", "ProcessNode"]],
          labels: { en: "expands capacity", ko: "증설" },
        },
      ],
    },
  };
}

export class MockWorldSchema {
  private readonly views = new Map<string, WorldSchemaView>();
  private readonly handlers = new Set<(event: { loupistId: string }) => void>();

  install(loupistId: string, view: WorldSchemaView) {
    this.views.set(loupistId, structuredClone(view));
  }

  get(loupistId: string): WorldSchemaView {
    return structuredClone(this.views.get(loupistId) ?? emptyWorldSchema());
  }

  apply(loupistId: string, proposalId: string): WorldSchemaView {
    const current = this.views.get(loupistId);
    const proposal = current?.proposal;
    if (!current || !proposal || proposal.proposalId !== proposalId || proposal.status !== "pending") {
      throw new Error("That suggestion is no longer available.");
    }
    const types = [...current.extension.types];
    for (const type of proposal.types) if (!types.some(item => item.name === type.name)) types.push(type);
    const relations = [...current.extension.relations];
    for (const relation of proposal.relations) if (!relations.some(item => item.name === relation.name)) relations.push(relation);
    if (types.length > 4 || relations.length > 6) throw new Error("This Loupist is already tracking as many kinds as it can.");
    return this.commit(loupistId, {
      extension: { version: current.extension.version + 1, types, relations, updatedAtMs: Date.now() },
      proposal: { ...proposal, status: "applied", decidedAtMs: Date.now() },
    });
  }

  dismiss(loupistId: string, proposalId: string): WorldSchemaView {
    const current = this.views.get(loupistId);
    const proposal = current?.proposal;
    if (!current || !proposal || proposal.proposalId !== proposalId || (proposal.status !== "pending" && proposal.status !== "preparing")) {
      throw new Error("That suggestion is no longer available.");
    }
    return this.commit(loupistId, {
      extension: current.extension,
      proposal: { ...proposal, status: "dismissed", decidedAtMs: Date.now() },
    });
  }

  remove(loupistId: string, kind: SchemaRemovalKind, name: string): WorldSchemaView {
    const current = this.views.get(loupistId) ?? emptyWorldSchema();
    let types = current.extension.types;
    let relations = current.extension.relations;
    if (kind === "type") {
      if (!types.some(type => type.name === name)) throw new Error("That type is not tracked.");
      types = types.filter(type => type.name !== name);
      relations = relations.flatMap(relation => {
        const signatures = relation.signatures.filter(([source, target]) => source !== name && target !== name);
        return signatures.length ? [{ ...relation, signatures }] : [];
      });
    } else {
      if (!relations.some(relation => relation.name === name)) throw new Error("That relationship is not tracked.");
      relations = relations.filter(relation => relation.name !== name);
    }
    return this.commit(loupistId, {
      extension: { version: current.extension.version + 1, types, relations, updatedAtMs: Date.now() },
      proposal: current.proposal,
    });
  }

  subscribe(handler: (event: { loupistId: string }) => void) {
    this.handlers.add(handler);
    return () => { this.handlers.delete(handler); };
  }

  private commit(loupistId: string, view: WorldSchemaView) {
    const stored = structuredClone(view);
    this.views.set(loupistId, stored);
    for (const handler of this.handlers) handler({ loupistId });
    return structuredClone(stored);
  }
}
