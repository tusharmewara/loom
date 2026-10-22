diff --git a/AGENTS.md b/AGENTS.md
index e544a2e..75f6561 100644
--- a/AGENTS.md
+++ b/AGENTS.md
@@ -559,3 +559,4 @@ Key constants in `loom-core/src/constants.rs`:
 | Vault decrypt | <10ms | ~3ms |
 | Knowledge query (subgraph, depth 2) | <20ms | ~5ms |
 | Knowledge build (100 sessions) | <5s | ~10ms (0 sessions with data) |
+- When a test in `crates/loom-hindsight` fails on fact importance, note that the importance value is a score bounded to the range 0.0 ..= 1.0: both ends are clamped, so a negative importance becomes 0.0 rather than staying negative.
