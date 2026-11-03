# Agent development

This page owns prompt, tool-contract and context work. [Architecture](runtime/chat-turns.md#chat)
owns runtime authority; [Bundled capabilities](bundled-capabilities.md) owns connection
and Skill management. These are developer instructions, not text to inject into Chat.

## Review the effective instructions

Read the sealed prefix the turn will actually send. [Prompt and judgment](agent/prompt.md#review-the-effective-instructions).

## Tool contracts and turn preparation

Catalog, evidence rules and native tool descriptions: [Tool catalog](agent/tools.md#tool-contracts-and-turn-preparation). Setup, Memory tools, assets and schedules: [Turn tools](agent/turn-tools.md).

## Investment judgment

State the evidence. Do not prescribe a buy, sell, size or target. [Prompt and judgment](agent/prompt.md#investment-judgment).

## Memory and context

What a turn is allowed to recall. What may be saved: [Context delivery](agent/context-delivery.md#memory-and-context). Bounds: [Context bounds](agent/context.md#context-assembly-and-bounds). Cutoff delivery: [Context delivery](agent/context-delivery.md).

### Context assembly and bounds

The sealed block order and byte caps. [Context bounds](agent/context.md#context-assembly-and-bounds).

## Scoped agent settings

Model and reasoning choices scoped to the Loupist or the Chat. [Prompt and judgment](agent/prompt.md#scoped-agent-settings).

## Measure cache behavior

How to see whether a prefix still hits the provider cache. [Prompt and judgment](agent/prompt.md#measure-cache-behavior).

## Verify the user's completed task

Check the visible result the user asked for, not only the tests that already exist. [Prompt and judgment](agent/prompt.md#verify-the-users-completed-task).
