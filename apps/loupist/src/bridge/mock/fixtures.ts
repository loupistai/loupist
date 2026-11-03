import type { ChatThread, LoupistSummary } from "../../types";

import { DEFAULT_AGENT_SKILL_IDS as MOCK_AGENT_SKILLS } from "../../lib/agentSkills";

export const MOCK_LOUPISTS: LoupistSummary[] = [
  {
    id: "loupist-quality",
    name: "Quality Compounder",
    workspace_path: "/Users/research/Quality Compounder",
    philosophy:
      "Own high-quality businesses for the long term at sensible prices.",
    record_count: 42,
    updated_at: "2026-08-06T09:20:00+09:00",
    accent: "var(--gg-color-accent)",
    enabled_skill_ids: [...MOCK_AGENT_SKILLS],
    availability: { status: "available" },
  },
  {
    id: "loupist-value",
    name: "Contrarian Value",
    workspace_path: "/Users/research/Contrarian Value",
    philosophy:
      "Prioritize the gap between price and intrinsic value, with downside protection first.",
    record_count: 31,
    updated_at: "2026-08-03T16:40:00+09:00",
    accent: "var(--gg-color-accent)",
    enabled_skill_ids: [...MOCK_AGENT_SKILLS],
    availability: { status: "available" },
  },
  {
    id: "loupist-cycle",
    name: "Cycle Reader",
    workspace_path: "/Users/research/Cycle Reader",
    philosophy:
      "Read industry-cycle inflections through inventory, pricing, and capital spending.",
    record_count: 27,
    updated_at: "2026-07-29T11:15:00+09:00",
    accent: "var(--gg-color-accent)",
    enabled_skill_ids: [...MOCK_AGENT_SKILLS],
    availability: { status: "available" },
  },
];

export const MOCK_THREADS: Record<string, ChatThread[]> = {
  "loupist-quality": [
    {
      id: "thread-margin",
      loupist_id: "loupist-quality",
      title: "How should we read the margin decline?",
      updated_at: "2026-08-07T10:42:00+09:00",
      use_memory: true,
      update_memory: true,
      messages: [
        {
          id: "msg-welcome-quality",
          role: "assistant",
          content:
            "Want to determine whether this quarter's margin decline is temporary? With Loupist Memory enabled, I can compare it with the quality criteria we established earlier.",
          created_at: "2026-08-07T10:40:00+09:00",
          status: "complete",
        },
      ],
    },
    {
      id: "thread-capital",
      loupist_id: "loupist-quality",
      title: "Capital allocation checklist",
      updated_at: "2026-08-04T14:10:00+09:00",
      use_memory: true,
      update_memory: true,
      messages: [],
    },
  ],
  "loupist-value": [
    {
      id: "thread-downside",
      loupist_id: "loupist-value",
      title: "Downside scenario review",
      updated_at: "2026-08-03T16:40:00+09:00",
      use_memory: true,
      update_memory: true,
      messages: [],
    },
  ],
  "loupist-cycle": [
    {
      id: "thread-inventory",
      loupist_id: "loupist-cycle",
      title: "Reading the inventory cycle",
      updated_at: "2026-07-29T11:15:00+09:00",
      use_memory: true,
      update_memory: true,
      messages: [],
    },
  ],
};
