#!/usr/bin/env python3
"""Contract tests for routing_hook.py (routing-debt.md Step 2a).

Pins CURRENT behavior — including quirks — before anything changes.
Plain asserts, no harness: python3 test_routing_hook.py
Rules files always live in a tempdir; ~/.loom is never touched.
"""

import io
import json
import os
import sys
import tempfile
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import routing_hook

PAYLOAD = json.dumps({
    "event_type": "PreToolUse",
    "payload": {"tool_name": "jcode", "command": "edit", "file_path": "src/server.py"},
})


def run_hook(stdin_text, rules_toml=None):
    """Run main() with faked stdin and a temp rules file (or none).

    Returns (stdout, stderr, exit_code)."""
    tmpdir = tempfile.mkdtemp()
    if rules_toml is not None:
        rules_path = os.path.join(tmpdir, "routing-rules.toml")
        with open(rules_path, "w") as f:
            f.write(rules_toml)
        routing_hook.RULES_PATH = Path(rules_path)
    else:
        routing_hook.RULES_PATH = Path(tmpdir) / "missing-rules.toml"

    old_stdin = sys.stdin
    sys.stdin = io.StringIO(stdin_text)
    out, err = io.StringIO(), io.StringIO()
    code = 0
    try:
        with redirect_stdout(out), redirect_stderr(err):
            routing_hook.main()
    except SystemExit as e:
        code = e.code
    finally:
        sys.stdin = old_stdin
    return out.getvalue(), err.getvalue(), code


def test_empty_stdin_is_noop():
    # Quirk pinned as-is: on empty stdin the "{}" goes to STDERR (stdout
    # stays empty) and the hook exits 0.
    out, err, code = run_hook("")
    assert code == 0, f"exit {code}"
    assert out == "", f"stdout {out!r}"
    assert err.strip() == "{}", f"stderr {err!r}"


def test_missing_rules_file_is_noop():
    out, err, code = run_hook(PAYLOAD)
    assert code == 0, f"exit {code}"
    assert json.loads(out) == {}
    assert err == ""


def test_malformed_rules_file_is_noop():
    # Bad TOML must degrade to "no rules" — never crash the tool call.
    out, err, code = run_hook(PAYLOAD, rules_toml="] not valid toml [[[")
    assert code == 0, f"exit {code}"
    assert json.loads(out) == {}
    assert "Error loading routing rules" in err


def test_rule_hit_adjusts_tier():
    rules = """
[[rules]]
pattern = "src/*.py"
command = "edit"
tier = "flagship"
reason = "big refactor on python"
"""
    out, err, code = run_hook(PAYLOAD, rules_toml=rules)
    assert code == 0, f"exit {code}"
    assert json.loads(out) == {"adjust_tier": "Flagship", "reason": "big refactor on python"}


def test_last_match_wins():
    # Both rules match; the LAST one in the file decides (main.py:104-108).
    rules = """
[[rules]]
pattern = "src/*.py"
tier = "slm"
reason = "first"

[[rules]]
pattern = "src/*.py"
tier = "flagship"
reason = "second"
"""
    out, _, code = run_hook(PAYLOAD, rules_toml=rules)
    assert code == 0, f"exit {code}"
    assert json.loads(out) == {"adjust_tier": "Flagship", "reason": "second"}


def test_min_complexity_gates_match():
    rules = """
[[rules]]
pattern = "src/*.py"
min_complexity = 0.7
tier = "flagship"
reason = "heavy"
"""
    below = dict(json.loads(PAYLOAD))
    below["payload"] = dict(below["payload"], complexity_score={"numeric": 0.4})
    out, _, code = run_hook(json.dumps(below), rules_toml=rules)
    assert code == 0 and json.loads(out) == {}

    above = dict(json.loads(PAYLOAD))
    above["payload"] = dict(above["payload"], complexity_score={"numeric": 0.8})
    out, _, code = run_hook(json.dumps(above), rules_toml=rules)
    assert code == 0
    assert json.loads(out) == {"adjust_tier": "Flagship", "reason": "heavy"}


def test_invalid_json_exits_1_with_empty_object():
    out, err, code = run_hook("this is not json")
    assert code == 1, f"exit {code}"
    assert json.loads(out) == {}
    assert "Error:" in err


def main():
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for t in tests:
        t()
        print(f"PASS {t.__name__}")
    print(f"{len(tests)} passed")


if __name__ == "__main__":
    main()
