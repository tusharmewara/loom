diff --git a/AGENTS.md b/AGENTS.md
index e544a2e..d901957 100644
--- a/AGENTS.md
+++ b/AGENTS.md
@@ -559,3 +559,4 @@ Key constants in `loom-core/src/constants.rs`:
 | Vault decrypt | <10ms | ~3ms |
 | Knowledge query (subgraph, depth 2) | <20ms | ~5ms |
 | Knowledge build (100 sessions) | <5s | ~10ms (0 sessions with data) |
+- When a test in `crates/loom-rules` fails on a comparison rule, note that the `Contains` operator asks whether the ACTUAL value contains the EXPECTED value — the operand order is `actual.contains(expected)`, not the reverse.
