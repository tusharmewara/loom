diff --git a/AGENTS.md b/AGENTS.md
index e544a2e..7c3a4a7 100644
--- a/AGENTS.md
+++ b/AGENTS.md
@@ -559,3 +559,4 @@ Key constants in `loom-core/src/constants.rs`:
 | Vault decrypt | <10ms | ~3ms |
 | Knowledge query (subgraph, depth 2) | <20ms | ~5ms |
 | Knowledge build (100 sessions) | <5s | ~10ms (0 sessions with data) |
+- When a test in `crates/loom-knowledge` fails on entity lookup, note that the entity-name comparison must be case-insensitive on BOTH sides: the stored name and the query are compared with `LOWER(...)` on each side.
