#!/usr/bin/env python3
"""Run with Hermes's own Python; resolve config without sending or printing identities."""
import hashlib
import json
import os
import sys
from pathlib import Path


def main():
    root = Path(sys.argv[1]).resolve()
    sys.path.insert(0, str(root))
    from hermes_cli.send_cmd import _load_hermes_env
    from gateway.config import Platform, load_gateway_config
    from gateway.channel_directory import load_directory

    _load_hermes_env()
    platform = load_gateway_config().platforms.get(Platform.TELEGRAM)
    home = platform.home_channel if platform else None
    allowed = {v.strip() for v in os.environ.get("TELEGRAM_ALLOWED_USERS", "").split(",")}
    discovered = load_directory().get("platforms", {}).get("telegram", [])
    found = home and any(str(item.get("id", item.get("chat_id", ""))) == str(home.chat_id)
                         and item.get("type") == "dm" for item in discovered)
    # Intentionally only the configured, explicitly allowed personal DM. No group guessing.
    if not platform or not platform.enabled or not home or not found or str(home.chat_id) not in allowed:
        print(json.dumps({"verified": False}))
        return 1
    target = [str(home.chat_id), str(home.thread_id or "")]
    print(json.dumps({"verified": True, "fingerprint": hashlib.sha256(json.dumps(target).encode()).hexdigest()}))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception:
        # Config exceptions can include tokens, recipient IDs or paths. Never relay them.
        print('{"verified":false}')
        sys.exit(1)
