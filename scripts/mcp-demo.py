#!/usr/bin/env python3
"""A small MCP server over stdio that asks the owner things while its tools run (T67, `elicitation/create`).

For trying Glut's forms by hand, nothing but Python's standard library:

    /mcp add demo -- python3 /path/to/scripts/mcp-demo.py

Its tools:
    ask_form    a form with every kind of field; the answer comes back as the tool's text
    ask_page    a page to open (https://example.com/)
    ask_secret  a form with a password field, which Glut must decline without showing it
    ask_often   asks seven times in one call; Glut asks the owner five times at most, none after a no
"""

import json
import sys

PROTOCOL_VERSION = "2025-11-25"

TOOLS = [
    {
        "name": "ask_form",
        "description": "Asks the owner a form and returns what they sent.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "ask_page",
        "description": "Asks the owner to open a page.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "ask_secret",
        "description": "Asks the owner a form with a password field.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "ask_often",
        "description": "Asks the owner a short form seven times in one call.",
        "inputSchema": {"type": "object", "properties": {}},
    },
]

FORM = {
    "message": "The demo server would like to know a few things.",
    "requestedSchema": {
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "title": "Name",
                "minLength": 2,
                "description": "What you are called.",
            },
            "mail": {"type": "string", "title": "E-mail", "format": "email"},
            "day": {"type": "string", "title": "A day", "format": "date"},
            "age": {"type": "integer", "title": "Age", "minimum": 0, "maximum": 150},
            "news": {"type": "boolean", "title": "Newsletter", "default": False},
            "size": {
                "type": "string",
                "title": "Size",
                "default": "m",
                "oneOf": [
                    {"const": "s", "title": "Small"},
                    {"const": "m", "title": "Medium"},
                    {"const": "l", "title": "Large"},
                ],
            },
            "tags": {
                "type": "array",
                "title": "Tags",
                "maxItems": 2,
                "items": {
                    "anyOf": [
                        {"const": "rust", "title": "Rust"},
                        {"const": "swift", "title": "Swift"},
                        {"const": "svelte", "title": "Svelte"},
                    ]
                },
            },
        },
        "required": ["name", "size"],
    },
}

PAGE = {
    "mode": "url",
    "message": "Please open the demo page.",
    "url": "https://example.com/",
    "elicitationId": "demo-page",
}

SECRET = {
    "message": "Sign in to the demo service.",
    "requestedSchema": {
        "type": "object",
        "properties": {"password": {"type": "string", "title": "Password"}},
        "required": ["password"],
    },
}

SHORT = {
    "message": "Still there?",
    "requestedSchema": {
        "type": "object",
        "properties": {"ok": {"type": "boolean", "title": "Yes", "default": True}},
    },
}


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def read():
    """The next JSON message from the client; None once it is gone."""
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            continue
    return None


class Server:
    def __init__(self):
        self.next_question = 0
        self.can_ask = False

    def ask(self, params):
        """Asks the client and waits for its answer; what else comes meanwhile is answered or let pass."""
        self.next_question += 1
        question = f"q{self.next_question}"
        send(
            {
                "jsonrpc": "2.0",
                "id": question,
                "method": "elicitation/create",
                "params": params,
            }
        )
        while True:
            message = read()
            if message is None:
                sys.exit(0)
            if message.get("id") == question and "method" not in message:
                return message.get("result") or {"error": message.get("error")}
            # A cancel of the call under way: the question will never be answered.
            if message.get("method") == "notifications/cancelled":
                return {"action": "cancelled by the client"}

    def call(self, name):
        if not self.can_ask:
            return "Glut did not say it can show forms (no elicitation capability)."
        if name == "ask_form":
            return json.dumps(self.ask(FORM), ensure_ascii=False)
        if name == "ask_page":
            return json.dumps(self.ask(PAGE), ensure_ascii=False)
        if name == "ask_secret":
            return json.dumps(self.ask(SECRET), ensure_ascii=False)
        if name == "ask_often":
            answers = [self.ask(SHORT).get("action") for _ in range(7)]
            return "seven questions, the answers: " + ", ".join(
                str(answer) for answer in answers
            )
        return f"no tool named {name}"

    def serve(self):
        while True:
            message = read()
            if message is None:
                return
            method, id_ = message.get("method"), message.get("id")
            if id_ is None:
                continue
            if method == "initialize":
                capabilities = message.get("params", {}).get("capabilities", {})
                self.can_ask = "elicitation" in capabilities
                result = {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "glut-demo", "version": "1"},
                }
            elif method == "tools/list":
                result = {"tools": TOOLS}
            elif method == "tools/call":
                text = self.call(message.get("params", {}).get("name", ""))
                result = {"content": [{"type": "text", "text": text}]}
            elif method == "ping":
                result = {}
            else:
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": id_,
                        "error": {"code": -32601, "message": "unknown method"},
                    }
                )
                continue
            send({"jsonrpc": "2.0", "id": id_, "result": result})


if __name__ == "__main__":
    Server().serve()
