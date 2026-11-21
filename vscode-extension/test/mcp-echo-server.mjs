// Minimal stdio MCP server used by the resume end-to-end extension tests.
// Implements initialize, tools/list and tools/call so the embedded server's
// MCP discovery and gate logic can run without a real MCP ecosystem.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { createInterface } from "node:readline";

const state = {
  tools: [
    {
      name: "echo",
      description: "Echo back the text given to it.",
      inputSchema: {
        type: "object",
        properties: { text: { type: "string", description: "text to echo" } },
        required: ["text"],
      },
    },
  ],
};

function respond(id, payload) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, ...payload }) + "\n");
}

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of rl) {
  let raw;
  try {
    raw = JSON.parse(line);
  } catch {
    continue;
  }
  const id = raw.id;
  if (raw.method === "initialize") {
    respond(id, { protocolVersion: "0.1.0", capabilities: {}, serverInfo: { name: "mcp-echo-test", version: "0.0.1" } });
  } else if (raw.method === "notifications/initialized") {
    // nothing
  } else if (raw.method === "tools/list") {
    respond(id, { tools: state.tools });
  } else if (raw.method === "tools/call") {
    const params = raw.params || {};
    const name = params.name || "";
    if (name !== "echo") {
      respond(id, { content: [{ type: "text", text: `unknown tool: ${name}` }], isError: true });
      continue;
    }
    const text = String(params.arguments && params.arguments.text !== undefined ? params.arguments.text : "");
    respond(id, {
      content: [{ type: "text", text: `ECHO:${text}` }],
    });
  } else {
    respond(id, { error: { message: `unhandled method ${raw.method}` } });
  }
}
