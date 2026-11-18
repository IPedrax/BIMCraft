# Proposed wiki addition: SolidCraft-backed families

Status: opt-in review feature; a separate SolidCraft 0.44.1 CLI is required. See the repository's
[SolidCraft bridge guide](../solidcraft-bridge.md) for the exact setup, examples, behavior matrix and limits.

A family carries a saved source document and explicit parameter bindings. The author chooses category,
Properties group and type/instance scope. BIMCraft Rust owns the BIM values; SolidCraft owns the modeling
rules. Native Properties, Edit Type and the semantic commands use the same family evaluation path.

The five original example sources demonstrate two cabinet layouts, a wall-hosted window, a wall-hosted door
and a decorative lantern. They do not establish support for every SolidCraft feature, material transfer,
full host/symbol behavior, electrical connectors or photometric simulation. The external publisher is a CLI;
a native mapping dialog is not part of this proposal.

Keep geometry editing in SolidCraft and republish. Edit Family/save/reload preserves the source and inputs;
it does not convert the source timeline into BIMCraft forms. Source input rename/delete is refused until a
reviewed republish. Pin the evaluator version, and use the review build to reopen source-backed projects.
