#!/usr/bin/env python3
"""Fake canonical ACP v1 agent for loom-sessions protocol client tests.

Speaks newline-delimited JSON-RPC 2.0 on stdio. Modes (--mode=...):
  happy       normal flow with chunks + a tool call, stopReason end_turn
  permission  asks session/request_permission, expects an allow* choice
  bad-version answers initialize with protocolVersion 999
  hang        answers initialize/session-new, then sleeps on prompt
  error       answers session/prompt with a JSON-RPC error
"""

import json
import sys
import time


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def read_request():
    line = sys.stdin.readline()
    if not line:
        return None
    return json.loads(line)


def main():
    mode = "happy"
    for arg in sys.argv[1:]:
        if arg.startswith("--mode="):
            mode = arg.split("=", 1)[1]

    # 1. initialize
    msg = read_request()
    if msg is None:
        return 1
    version = 999 if mode == "bad-version" else 1
    send({"jsonrpc": "2.0", "id": msg.get("id"), "result": {
        "protocolVersion": version,
        "agentCapabilities": {},
    }})
    if mode == "bad-version":
        return 0

    # 2. session/new
    msg = read_request()
    if msg is None:
        return 1
    send({"jsonrpc": "2.0", "id": msg.get("id"), "result": {
        "sessionId": "sess-test-1",
    }})

    # 3. session/prompt
    msg = read_request()
    if msg is None:
        return 1
    prompt_id = msg.get("id")

    if mode == "hang":
        time.sleep(3600)
        return 0

    if mode == "error":
        send({"jsonrpc": "2.0", "id": prompt_id,
              "error": {"code": -32000,
                        "message": "Authentication required: provider authentication required"}})
        return 0

    if mode == "permission":
        send({"jsonrpc": "2.0", "id": 100,
              "method": "session/request_permission",
              "params": {
                  "sessionId": "sess-test-1",
                  "toolCall": {"toolCallId": "call-9"},
                  "options": [
                      {"optionId": "allow-once", "name": "Allow once",
                       "kind": "allow_once"},
                      {"optionId": "reject-once", "name": "Reject",
                       "kind": "reject_once"},
                  ],
              }})
        resp = read_request()
        outcome = (resp or {}).get("result", {}).get("outcome", {})
        if outcome.get("outcome") != "selected" or \
                outcome.get("optionId") != "allow-once":
            send({"jsonrpc": "2.0", "id": prompt_id,
                  "error": {"code": -32000, "message": "permission denied"}})
            return 0
        send({"jsonrpc": "2.0", "method": "session/update",
              "params": {"sessionId": "sess-test-1", "update": {
                  "sessionUpdate": "agent_message_chunk",
                  "content": {"type": "text", "text": "did the thing"},
              }}})
        send({"jsonrpc": "2.0", "id": prompt_id,
              "result": {"stopReason": "end_turn"}})
        return 0

    # happy path
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": "sess-test-1", "update": {
              "sessionUpdate": "agent_message_chunk",
              "content": {"type": "text", "text": "Hello "},
          }}})
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": "sess-test-1", "update": {
              "sessionUpdate": "agent_message_chunk",
              "content": {"type": "text", "text": "back"},
          }}})
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": "sess-test-1", "update": {
              "sessionUpdate": "tool_call",
              "toolCallId": "call-1",
              "title": "Reading file",
              "kind": "read",
              "status": "pending",
          }}})
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": "sess-test-1", "update": {
              "sessionUpdate": "tool_call_update",
              "toolCallId": "call-1",
              "status": "completed",
          }}})
    send({"jsonrpc": "2.0", "id": prompt_id,
          "result": {"stopReason": "end_turn"}})
    return 0


if __name__ == "__main__":
    sys.exit(main())
