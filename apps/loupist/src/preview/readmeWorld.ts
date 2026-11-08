import type { KnowledgeMap, KnowledgeMapCount, KnowledgeMapEdge, KnowledgeMapEdgeKind, KnowledgeMapNode, KnowledgeMapNodeKind } from "../contracts/knowledge";

const START = Date.UTC(2026, 6, 1);
const END = Date.UTC(2026, 9, 27);

const foundries = ["TSMC", "Samsung Electronics", "Intel", "GlobalFoundries", "UMC", "SMIC", "Tower Semiconductor", "Vanguard International Semiconductor", "Powerchip", "Hua Hong", "Rapidus", "Nexchip", "X-FAB", "SkyWater", "Win Semiconductors", "DB HiTek"];
const equipment = ["ASML", "Applied Materials", "Lam Research", "Tokyo Electron", "KLA", "SCREEN Holdings", "ASM International", "Onto Innovation", "Axcelis", "Veeco", "Teradyne", "Advantest", "Disco", "Nikon", "Canon", "Hitachi High-Tech", "FormFactor", "ASMPT", "BE Semiconductor", "Kulicke and Soffa"];
const materials = ["Shin-Etsu Chemical", "SUMCO", "Siltronic", "GlobalWafers", "SK Siltron", "Entegris", "Air Liquide", "Linde", "Merck KGaA", "JSR", "Tokyo Ohka Kogyo", "DuPont", "Resonac", "Mitsui Chemicals", "CMC Materials", "Soulbrain", "Hansol Chemical", "Dongjin Semichem"];
const memory = ["SK hynix", "Micron", "Kioxia", "Western Digital", "Nanya", "Winbond", "Macronix", "GigaDevice", "CXMT", "YMTC"];
const designers = ["NVIDIA", "Apple", "AMD", "Qualcomm", "Broadcom", "MediaTek", "Marvell", "Arm", "Tesla", "Sony", "Super Micro", "Dell", "Cisco", "Arista", "IBM", "Nokia", "Ericsson", "Nintendo"];
const cloud = ["Microsoft", "Amazon", "Alphabet", "Meta", "Oracle", "CoreWeave", "Equinix", "Digital Realty"];
const packaging = ["ASE", "Amkor", "JCET", "Ibiden", "Unimicron", "Shinko Electric", "AT&S", "Kinsus", "Powertech", "Nan Ya PCB"];
const eda = ["Synopsys", "Cadence", "Siemens EDA", "Ansys", "Rambus", "Alphawave", "Imagination", "Arteris", "Ceva"];
const analog = ["Texas Instruments", "Analog Devices", "Infineon", "NXP", "STMicroelectronics", "Renesas", "Microchip", "onsemi", "Wolfspeed", "Rohm", "Murata", "TDK", "Kyocera", "Samsung Electro-Mechanics", "LG Innotek", "Yageo"];
const processTools = ["Hanmi Semiconductor", "Wonik IPS", "Eugene Technology", "TES", "PSK", "Jusung Engineering", "HPSP", "ISC", "Leeno Industrial", "Koh Young", "Park Systems", "LOT Vacuum"];
const places = ["Taiwan", "South Korea", "Japan", "United States", "Netherlands", "China"];
const technologies = ["High bandwidth memory", "EUV lithography", "Advanced packaging", "CoWoS", "Data center GPUs", "Silicon carbide"];
const policies = ["Export controls"];

const kindOf = new Map<string, string>([
  ...technologies.map(name => [name, "Technology"] as const),
  ...places.map(name => [name, "Place"] as const),
  ...policies.map(name => [name, "Policy"] as const),
]);

const watched = new Set(["NVIDIA", "Apple"]);
const held = new Set(["TSMC", "SK hynix"]);

const rings: { relation: string; names: string[] }[] = [
  { relation: "COMPETES_WITH", names: foundries },
  { relation: "COMPETES_WITH", names: memory },
  { relation: "COMPETES_WITH", names: designers },
  { relation: "COMPETES_WITH", names: cloud },
  { relation: "COMPETES_WITH", names: analog },
  { relation: "COMPETES_WITH", names: equipment },
];

const fans: { relation: string; from: string[]; to: string[] }[] = [
  { relation: "SUPPLIES", from: equipment, to: ["TSMC", "Samsung Electronics", "Intel"] },
  { relation: "SUPPLIES", from: materials, to: ["TSMC", "Samsung Electronics"] },
  { relation: "SUPPLIES", from: memory, to: ["NVIDIA", "AMD", "Apple"] },
  { relation: "DEPENDS_ON", from: designers, to: ["TSMC"] },
  { relation: "DEPENDS_ON", from: cloud, to: ["NVIDIA"] },
  { relation: "SUPPLIES", from: packaging, to: ["TSMC", "Apple"] },
  { relation: "SUPPLIES", from: eda, to: ["NVIDIA", "Apple", "AMD"] },
  { relation: "SUPPLIES", from: analog, to: ["Apple", "Tesla", "Cisco"] },
  { relation: "SUPPLIES", from: processTools, to: ["Samsung Electronics", "SK hynix"] },
  { relation: "SUPPLIES", from: ["SK hynix", "Micron", "Samsung Electronics"], to: ["High bandwidth memory"] },
  { relation: "SUPPLIES", from: ["High bandwidth memory"], to: ["NVIDIA"] },
  { relation: "SUPPLIES", from: ["ASML"], to: ["EUV lithography"] },
  { relation: "SUPPLIES", from: ["EUV lithography"], to: ["TSMC"] },
  { relation: "SUPPLIES", from: ["ASE", "Amkor"], to: ["Advanced packaging"] },
  { relation: "SUPPLIES", from: ["TSMC"], to: ["CoWoS"] },
  { relation: "SUPPLIES", from: ["CoWoS"], to: ["NVIDIA"] },
  { relation: "SUPPLIES", from: ["NVIDIA"], to: ["Data center GPUs"] },
  { relation: "SUPPLIES", from: ["Data center GPUs"], to: ["Microsoft", "Amazon", "Alphabet", "Meta"] },
  { relation: "SUPPLIES", from: ["onsemi", "Wolfspeed"], to: ["Silicon carbide"] },
  { relation: "SUPPLIES", from: ["Silicon carbide"], to: ["Tesla"] },
  { relation: "HEADQUARTERED_IN", from: ["TSMC"], to: ["Taiwan"] },
  { relation: "HEADQUARTERED_IN", from: ["Samsung Electronics", "SK hynix"], to: ["South Korea"] },
  { relation: "HEADQUARTERED_IN", from: ["Tokyo Electron", "Rapidus"], to: ["Japan"] },
  { relation: "HEADQUARTERED_IN", from: ["ASML", "ASM International", "BE Semiconductor"], to: ["Netherlands"] },
  { relation: "HEADQUARTERED_IN", from: ["NVIDIA", "Apple", "Intel", "Applied Materials"], to: ["United States"] },
  { relation: "HEADQUARTERED_IN", from: ["SMIC"], to: ["China"] },
  { relation: "EXPOSED_TO", from: ["SMIC", "Hua Hong", "Nexchip"], to: ["Export controls"] },
];

function slug(name: string) {
  return `readme-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")}`;
}

function names() {
  const all = [...rings.flatMap(ring => ring.names), ...fans.flatMap(fan => [...fan.from, ...fan.to]), ...technologies, ...places, ...policies];
  const unique = [...new Set(all)];
  const ids = unique.map(slug);
  if (new Set(ids).size !== ids.length) throw new Error("readme world slugs collide");
  if (unique.length < 140) throw new Error(`readme world has ${unique.length} names, which leaves the map sparse`);
  return unique;
}

export function readmeWorldMap(): KnowledgeMap {
  const labels = names();
  const nodes: KnowledgeMapNode[] = labels.map((label, index) => ({
    id: slug(label),
    kind: "entity",
    label,
    entityKind: kindOf.get(label) ?? "Organization",
    degree: 0,
    watched: watched.has(label),
    held: held.has(label),
    firstSeenAtMs: START + Math.round(index / Math.max(1, labels.length - 1) * (END - START)),
  }));
  const byName = new Map(labels.map(label => [label, slug(label)]));
  const endpoint = (name: string) => {
    const id = byName.get(name);
    if (!id) throw new Error(`readme world is missing ${name}`);
    return id;
  };
  const edges: KnowledgeMapEdge[] = [];
  const seen = new Set<string>();
  const link = (from: string, to: string, relation: string, status: "active" | "disputed" = "active") => {
    if (from === to) return;
    const source = endpoint(from);
    const target = endpoint(to);
    const id = `${relation}:${source}:${target}`;
    if (seen.has(id)) return;
    seen.add(id);
    const seenAt = (id: string) => nodes.find(node => node.id === id)?.firstSeenAtMs ?? START;
    edges.push({ id: `readme-claim-${edges.length}`, kind: "claim", source, target, relation, status, firstSeenAtMs: Math.max(seenAt(source), seenAt(target)) });
  };
  for (const ring of rings) for (let index = 0; index < ring.names.length; index += 1) link(ring.names[index]!, ring.names[(index + 1) % ring.names.length]!, ring.relation);
  for (const fan of fans) for (const from of fan.from) for (const to of fan.to) link(from, to, fan.relation);
  const disputed = edges.find(edge => edge.relation === "EXPOSED_TO" && edge.source === slug("SMIC"));
  if (!disputed) throw new Error("readme world is missing the disputed export-control claim");
  disputed.status = "disputed";
  for (const edge of edges) for (const id of [edge.source, edge.target]) nodes.find(node => node.id === id)!.degree += 1;
  const count = (total: number): KnowledgeMapCount => ({ total, shown: total, truncated: false });
  const empty = count(0);
  const nodeKinds: KnowledgeMapNodeKind[] = ["entity", "lens_perspective", "lens_principle", "lens_method", "lens_subject", "lens_idea", "journal"];
  const edgeKinds: KnowledgeMapEdgeKind[] = ["claim", "lens_link", "applies_to", "about"];
  const counts = {} as KnowledgeMap["counts"];
  for (const kind of nodeKinds) counts[kind] = kind === "entity" ? count(nodes.length) : empty;
  for (const kind of edgeKinds) counts[kind] = kind === "claim" ? count(edges.length) : empty;
  return {
    nodes,
    edges,
    counts,
    unmatchedHoldings: [],
    unmatchedWatchlist: [],
    worldRevisionId: "readme-world",
    generatedAtMs: END,
  };
}
