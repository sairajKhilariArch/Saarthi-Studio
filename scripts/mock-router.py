#!/usr/bin/env python3
"""Minimal FreeLLMAPI stand-in for local UI and console testing.

It answers the endpoints Studio uses: the proxy half (`/api/ping`,
`/v1/chat/completions`) and the dashboard half (`/api/auth/*`, `/api/keys`,
`/api/models`, `/api/fallback/routing`, `/api/analytics/*`, `/api/logs`,
`/api/free-tier`). State is in memory, so it is a test fixture, not a router.

Usage: python3 scripts/mock-router.py [--port 3101] [--require-login]
"""

import argparse
import json
import re
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

STATE = {
    "sessions": set(),
    "keys": [
        {"id": 1, "platform": "groq", "label": "work", "enabled": True,
         "keyPreview": "gsk_…9f2c", "requestCap": 1000, "tokenCap": 0, "requests": 42},
        {"id": 2, "platform": "google", "label": "personal", "enabled": False,
         "keyPreview": "AIza…4b1d", "requestCap": 0, "tokenCap": 250000, "requests": 7},
    ],
    "models": [
        {"id": "auto", "platform": "router", "execution_status": "ready"},
        {"id": "llama-3.3-70b-versatile", "platform": "groq", "execution_status": "ready"},
        {"id": "gemini-2.5-flash", "platform": "google", "execution_status": "ready"},
        {"id": "gemini-2.5-pro", "platform": "google", "execution_status": "needsKey"},
        {"id": "deepseek-r1:8b", "platform": "ollama", "execution_status": "exhausted"},
    ],
    "routing": {
        "strategy": "balanced",
        "chain": ["gemini-2.5-flash", "llama-3.3-70b-versatile"],
        "exploreEnabled": True,
        "keySelectionStrategy": "auto",
    },
    "logs": [
        {"id": 3, "ts": "2026-10-24T06:10:00.000Z", "level": "info",
         "provider": "groq", "message": "routed chat request to llama-3.3-70b-versatile"},
        {"id": 2, "ts": "2026-10-24T06:09:40.000Z", "level": "warn",
         "provider": "google", "message": "rate limited; cooling down for 45s"},
        {"id": 1, "ts": "2026-10-24T06:09:12.000Z", "level": "error",
         "provider": "google", "message": "429 quota exceeded on free tier"},
    ],
}

ARGS = argparse.Namespace(require_login=False)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # keep the test output readable
        print(f"[mock-router] {self.command} {self.path}")

    # -- helpers -----------------------------------------------------
    def send_json(self, payload, status=200, extra_headers=None):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        for key, value in (extra_headers or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(body)

    def read_json(self):
        length = int(self.headers.get("Content-Length") or 0)
        if not length:
            return {}
        raw = self.rfile.read(length)
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            return {}

    def token(self):
        header = self.headers.get("Authorization") or ""
        token = header[7:] if header.lower().startswith("bearer ") else ""
        return token or (self.headers.get("x-dashboard-token") or "")

    def authorized(self):
        if not ARGS.require_login:
            return True
        return self.token() in STATE["sessions"]

    # -- routes ------------------------------------------------------
    def do_GET(self):
        path = self.path.split("?")[0]
        query = self.path.split("?", 1)[1] if "?" in self.path else ""

        if path == "/api/ping":
            return self.send_json({"status": "ok", "timestamp": "2026-10-24T06:00:00.000Z"})
        if path in ("/livez", "/readyz"):
            return self.send_json({"status": "ok"})

        if path == "/api/keys":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            return self.send_json({"keys": STATE["keys"]})
        if path == "/api/models":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            return self.send_json({"models": STATE["models"]})
        if path == "/v1/models":
            return self.send_json({"object": "list", "data": [{"id": m["id"]} for m in STATE["models"]]})
        if path == "/api/fallback/routing":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            return self.send_json(STATE["routing"])
        if path == "/api/analytics/summary":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            return self.send_json({
                "totalRequests": 128, "successRate": 0.945,
                "totalInputTokens": 412000, "totalOutputTokens": 96000,
                "avgLatencyMs": 812, "p50LatencyMs": 640, "p95LatencyMs": 2140,
                "estimatedCostSavings": 3.42, "pinnedRequests": 4,
            })
        if path == "/api/analytics/by-model":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            return self.send_json({"models": [
                {"model": "gemini-2.5-flash", "requests": 80, "failures": 2, "avgLatencyMs": 640},
                {"model": "llama-3.3-70b-versatile", "requests": 48, "failures": 5, "avgLatencyMs": 980},
            ]})
        if path == "/api/logs":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            entries = STATE["logs"]
            levels = re.search(r"levels=([^&]+)", query)
            if levels:
                wanted = levels.group(1).split(",")
                entries = [e for e in entries if e["level"] in wanted]
            return self.send_json({"entries": entries, "nextId": 4, "counts": {"error": 1, "warn": 1, "info": 1}})
        if path == "/api/free-tier":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            return self.send_json({
                "summary": {"poolCount": 2, "documentedMonthlyTokens": 120000000,
                            "creditsBasedPools": 0, "unpublishedPools": 1},
                "pools": [
                    {"poolKey": "google", "platform": "google", "modelCount": 2, "keyCount": 1,
                     "documentedBudget": 120000000, "bestLabel": "~120M", "kind": "documented",
                     "quota": {"limit": 150000000, "remaining": 98000000, "metric": "tokens"}},
                    {"poolKey": "groq", "platform": "groq", "modelCount": 1, "keyCount": 1,
                     "documentedBudget": 0, "bestLabel": "—", "kind": "credits", "quota": {}},
                ],
            })
        if path == "/api/auth/status":
            return self.send_json({"needsSetup": False, "authenticated": self.authorized()})
        return self.send_json({"error": "not found", "path": path}, 404)

    def do_POST(self):
        path = self.path.split("?")[0]

        if path == "/api/auth/desktop-session":
            if ARGS.require_login:
                return self.send_json({"error": "sign in required"}, 403)
            token = "session-desktop"
            STATE["sessions"].add(token)
            return self.send_json({"token": token, "mode": "desktop"})
        if path == "/api/auth/login":
            body = self.read_json()
            if body.get("email") and body.get("password"):
                token = "session-login"
                STATE["sessions"].add(token)
                return self.send_json({"token": token})
            return self.send_json({"error": "invalid credentials"}, 401)
        if path == "/api/keys":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            body = self.read_json()
            if not body.get("platform"):
                return self.send_json({"error": "platform is required"}, 400)
            entry = {
                "id": (max([k["id"] for k in STATE["keys"]] or [0]) + 1),
                "platform": body["platform"],
                "label": body.get("label", ""),
                "enabled": True,
                "keyPreview": (body.get("key", "")[:4] + "…") if body.get("key") else "",
                "requestCap": 0,
                "tokenCap": 0,
                "requests": 0,
            }
            STATE["keys"].append(entry)
            return self.send_json(entry, 201)
        if path == "/api/logs/clear":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            STATE["logs"] = []
            return self.send_json({"ok": True})
        if path == "/v1/chat/completions":
            body = self.read_json()
            model = body.get("model", "auto")
            return self.send_json({
                "id": "chatcmpl-mock", "object": "chat.completion", "model": model,
                "choices": [{"index": 0, "finish_reason": "stop",
                             "message": {"role": "assistant",
                                         "content": "Mock router reply. Context packed: "
                                                    + str(len(body.get("messages", []))) + " messages."}}],
                "usage": {"prompt_tokens": 120, "completion_tokens": 48, "total_tokens": 168},
                "provider": "mock", "fallback_attempts": "0",
            })
        return self.send_json({"error": "not found", "path": path}, 404)

    def do_PATCH(self):
        path = self.path.split("?")[0]
        match = re.match(r"^/api/keys/(\d+)$", path)
        if not match:
            return self.send_json({"error": "not found"}, 404)
        if not self.authorized():
            return self.send_json({"error": "unauthorized"}, 401)
        body = self.read_json()
        for key in STATE["keys"]:
            if key["id"] == int(match.group(1)):
                key.update({k: v for k, v in body.items() if k in ("enabled", "label")})
                return self.send_json(key)
        return self.send_json({"error": "key not found"}, 404)

    def do_PUT(self):
        path = self.path.split("?")[0]
        if path == "/api/fallback/routing":
            if not self.authorized():
                return self.send_json({"error": "unauthorized"}, 401)
            body = self.read_json()
            if not body.get("strategy"):
                return self.send_json({"error": "strategy is required"}, 400)
            STATE["routing"].update(body)
            return self.send_json(STATE["routing"])
        return self.send_json({"error": "not found"}, 404)

    def do_DELETE(self):
        path = self.path.split("?")[0]
        match = re.match(r"^/api/keys/(\d+)$", path)
        if not match:
            return self.send_json({"error": "not found"}, 404)
        if not self.authorized():
            return self.send_json({"error": "unauthorized"}, 401)
        before = len(STATE["keys"])
        STATE["keys"][:] = [k for k in STATE["keys"] if k["id"] != int(match.group(1))]
        if len(STATE["keys"]) == before:
            return self.send_json({"error": "key not found"}, 404)
        return self.send_json({"ok": True})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=3101)
    parser.add_argument("--require-login", action="store_true")
    parsed = parser.parse_args()
    ARGS.require_login = parsed.require_login
    server = ThreadingHTTPServer(("127.0.0.1", parsed.port), Handler)
    print(f"mock FreeLLMAPI listening on http://127.0.0.1:{parsed.port} (login required: {ARGS.require_login})")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
