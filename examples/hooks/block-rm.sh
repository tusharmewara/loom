#!/bin/sh
# PreToolUse: block destructive rm commands (ecosystem example hook)
case "${LOOM_MATCHER:-}" in
  *rm\ -rf*|*rm\ -fr*) exit 1 ;;
esac
exit 0
