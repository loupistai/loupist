export function isKoreanUserTurn(text: string): boolean;
export function expectations(turn: { expect?: Record<string, unknown[]> }): Array<{ bucket: string; item: Record<string, unknown> }>;
export function userCorpus(scenario: { sessions?: Array<{ turns?: Array<{ user?: string; attachments?: Array<{ text?: string }> }> }>; workspace_files?: Record<string, string> }): string;
export const toolNames: Set<string>;
export function assertScenarioContract(scenario: object, options?: { suite?: string; label?: string; seenIds?: Map<string, string> }): void;
