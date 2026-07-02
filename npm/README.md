# @anthropic/infigraph

npm wrapper for [infigraph](https://github.com/intuit/infigraph) — code intelligence graph with AST parsing, semantic search, knowledge graph, and MCP server.

## Install

```bash
npm install -g @anthropic/infigraph
```

This downloads the pre-built native binary for your platform.

## Usage

```bash
# Index a project
infigraph index

# Start MCP server
infigraph-mcp
```

## MCP Configuration

Add to your Claude Desktop or Claude Code config:

```json
{
  "mcpServers": {
    "infigraph": {
      "command": "npx",
      "args": ["@anthropic/infigraph-mcp"]
    }
  }
}
```

## Corporate Network

If behind a corporate firewall, set `INFIGRAPH_MIRROR` to your internal artifact mirror:

```bash
INFIGRAPH_MIRROR=https://artifact.example.com/infigraph npm install -g @anthropic/infigraph
```

## Migration from Terragraph

If you previously used Terragraph, the installer automatically:
1. Removes the terragraph binary and MCP config
2. Migrates `.terragraph/` data directories to `.infigraph/`
3. Keeps old `.terragraph/` as backup

## Supported Platforms

| Platform | Architecture |
|----------|-------------|
| macOS | arm64 (Apple Silicon), x64 (Intel) |
| Windows | x64 |

## License

Apache-2.0
