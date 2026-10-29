# Product lens

Score: 4/10 | Vote: PASS

## What is strong
- End-to-end control: daemon + CLI + TUI + MCP server + adapters is vertical integration; nothing is outsourced to a middle layer.
- "It just works" for the technical user: local daemon, offline-capable, auto-starts, Unix socket IPC — seamless once running.

## What would kill it (top 3, most dangerous first)
1. Invisible product: Loom is plumbing. Jobsian products win on felt experience and visible design; a background daemon that shares context has no UI surface to delight, so it cannot inspire.
2. Violates focus: 32 crates, 11 agents, license gate, vault, SSO, FIPS, audit, analytics — breadth over refinement. Jobs said no to 1000 things; Loom says yes to almost everything.
3. Buyer is not the user: enterprise procurement (SSO/FIPS/audit) decides, not the developer living in the product. Delighting the developer does not win the sale; checking compliance boxes does. That is not a Jobs product model.

## The questions I would ask the founder
- What does a user feel in the first 60 seconds that makes them show a colleague?
- Which of the 11 agents and 8 commercialization features can be cut to sharpen the product?

## If FUND IF: the conditions
None — vote is PASS.
