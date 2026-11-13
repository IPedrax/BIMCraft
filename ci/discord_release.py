#!/usr/bin/env python3
"""Posts a GitHub release to a Discord webhook: its notes as an embed, and a ping for one role.

    gh release view TAG --json name,url,body | python3 ci/discord_release.py WEBHOOK_URL [ROLE_ID]
    python3 ci/discord_release.py --self-test

The release workflow runs it after the release exists. A bad webhook or a Discord outage makes it fail, which that job allows.
"""
import json
import sys
import urllib.request

LIMIT = 3500  # an embed description holds 4096 characters; the cut line and its link need the rest
HOSTS = ("https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/")


def payload(release, role=""):
    role = role.strip() if role.strip().isdigit() else ""  # only an id, so the secret cannot add other mentions
    notes = release.get("body") or ""
    if len(notes) > LIMIT:
        notes = notes[:LIMIT].rsplit("\n", 1)[0] + f"\n\n[Read the rest on GitHub]({release['url']})"
    out = {
        "embeds": [{"title": release["name"], "url": release["url"], "description": notes, "color": 0x167A63}],
        "allowed_mentions": {"roles": [role] if role else []},
    }
    if role:
        out["content"] = f"<@&{role}> {release['name']} is out."
    return out


def post(url, data):
    if not url.startswith(HOSTS):
        sys.exit("The webhook is not a Discord webhook address.")
    # Discord refuses urllib's own User-Agent.
    request = urllib.request.Request(
        url,
        json.dumps(data).encode(),
        {"Content-Type": "application/json", "User-Agent": "DiscordBot (https://github.com/IPedrax/BIMCraft, 1)"},
    )
    urllib.request.urlopen(request, timeout=30).read()


def self_test():
    release = {"name": "BIMCraft v1.2.3", "url": "https://example.test/r", "body": "## What's Changed\n* a fix"}
    out = payload(release, "123456789")
    assert out["content"] == "<@&123456789> BIMCraft v1.2.3 is out."
    assert out["allowed_mentions"] == {"roles": ["123456789"]}
    assert out["embeds"][0]["description"] == release["body"]
    for bad in ("", "  ", "everyone", "1> @everyone"):
        out = payload(release, bad)
        assert "content" not in out and out["allowed_mentions"] == {"roles": []}, bad
    long = dict(release, body="\n".join(f"* change {n}" for n in range(2000)))
    text = payload(long)["embeds"][0]["description"]
    assert len(text) <= 4096 and text.endswith("(https://example.test/r)") and "change 1999" not in text
    assert payload(dict(release, body=None))["embeds"][0]["description"] == ""
    print("ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    elif len(sys.argv) in (2, 3):
        post(sys.argv[1], payload(json.load(sys.stdin), sys.argv[2] if len(sys.argv) == 3 else ""))
    else:
        sys.exit(__doc__)
