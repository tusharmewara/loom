#!/usr/bin/env python3
"""
Routing Hook for Loom — Per-Call Tier Adjustment (Stage 3)

This hook is triggered on PreToolUse events. It inspects the upcoming tool
invocation (file path, command, operation) and applies any matching rules
from ~/.agents/config/routing-rules.toml to override the routing tier for
this specific call.

Input (stdin): JSON object representing HookContext:
{
  "event_type": "PreToolUse",
  "payload": {
    "tool_name": "jcode",
    "command": "edit",
    "file_path": "src/server.py",
    "task_id": "abc123",
    "complexity_score": { "level": "Medium", "numeric": 0.62 }
  },
  "tool_name": "jcode"
}

Output (stdout): JSON object indicating adjustment:
{
  "adjust_tier": "Flagship",
  "reason": "Rule: refactor on *.py (tier=Flagship)"
}

If no rule matches, output {} (empty object).
"""

import sys
import json
import toml
from pathlib import Path

AGENTS_DIR = Path.home() / ".agents"
RULES_PATH = AGENTS_DIR / "config" / "routing-rules.toml"

def load_rules():
    if not RULES_PATH.exists():
        return []
    try:
        data = toml.load(RULES_PATH)
        return data.get("rules", [])
    except Exception as e:
        print(f"Error loading routing rules: {e}", file=sys.stderr)
        return []

def match_rule(rule, context):
    # Match file pattern (glob) against file_path if present
    if "pattern" in rule:
        pattern = rule["pattern"]
        file_path = context.get("file_path", "")
        if file_path:
            from fnmatch import fnmatch
            if not fnmatch(file_path, pattern):
                return False
        else:
            # If rule expects a file pattern but no file_path, no match
            return False

    # Match command (exact or substring)
    if "command" in rule:
        wanted = rule["command"]
        actual = context.get("command", "")
        if wanted != actual:
            return False

    # Match tool_name
    if "tool_name" in rule:
        wanted = rule["tool_name"]
        actual = context.get("tool_name", "")
        if wanted != actual:
            return False

    # Match operation (could be alias for command)
    if "operation" in rule:
        wanted = rule["operation"]
        actual = context.get("command", "")
        if wanted != actual:
            return False

    # Minimal complexity threshold: if rule has min_complexity and payload has numeric
    if "min_complexity" in rule:
        min_val = rule["min_complexity"]
        comp = context.get("complexity_score", {}).get("numeric", 0)
        if comp < min_val:
            return False

    # All specified conditions matched
    return True

def main():
    try:
        input_str = sys.stdin.read()
        if not input_str.strip():
            print("{}", file=sys.stderr)
            sys.exit(0)
        context = json.loads(input_str)
        payload = context.get("payload", {})

        rules = load_rules()
        # Apply highest-priority rule (last in list wins if multiple match)
        matched_rule = None
        for rule in rules:
            if match_rule(rule, payload):
                matched_rule = rule

        if matched_rule:
            tier = matched_rule.get("tier", "").capitalize()
            reason = matched_rule.get("reason") or f"Rule matched: {matched_rule}"
            output = {"adjust_tier": tier, "reason": reason}
            print(json.dumps(output))
        else:
            print("{}")
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        print("{}")
        sys.exit(1)

if __name__ == "__main__":
    main()
