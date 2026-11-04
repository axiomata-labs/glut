"""The shop's stock as an MCP server over stdio: the demo's own, no network, no files."""

import json
import sys

STOCK = {
    "MUG-RED": ("Big red mug", 14),
    "MUG-BLUE": ("Blue mug", 3),
    "CUP-SET": ("Cup set of four", 0),
    "PLATE-L": ("Large plate", 22),
    "PLATE-S": ("Small plate", 4),
    "BOWL": ("Soup bowl", 9),
    "TEAPOT": ("Teapot", 1),
    "SAUCER": ("Saucer", 31),
}

TOOLS = [
    {
        "name": "products",
        "description": "Every product of the shop: its SKU and name.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "stock_level",
        "description": "How many of one product are in stock.",
        "inputSchema": {
            "type": "object",
            "properties": {"sku": {"type": "string", "description": "The product's SKU, e.g. MUG-RED"}},
            "required": ["sku"],
        },
    },
]


def call(name, arguments):
    if name == "products":
        return [{"sku": sku, "name": named} for sku, (named, _) in STOCK.items()]
    if name == "stock_level":
        sku = arguments.get("sku", "")
        if sku not in STOCK:
            raise ValueError(f"no product {sku}")
        return {"sku": sku, "level": STOCK[sku][1]}
    raise ValueError(f"no tool {name}")


def answer(request):
    method, params = request.get("method"), request.get("params") or {}
    if method == "initialize":
        return {
            "protocolVersion": params.get("protocolVersion", "2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "stock", "version": "1.0"},
        }
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        try:
            result = call(params.get("name"), params.get("arguments") or {})
            return {"content": [{"type": "text", "text": json.dumps(result)}]}
        except ValueError as err:
            return {"content": [{"type": "text", "text": str(err)}], "isError": True}
    if method == "ping":
        return {}
    raise LookupError(method)


def main():
    for line in sys.stdin:
        request = json.loads(line)
        if "id" not in request:
            continue
        try:
            reply = {"jsonrpc": "2.0", "id": request["id"], "result": answer(request)}
        except LookupError as err:
            reply = {"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": f"no method {err}"}}
        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
