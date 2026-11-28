# Using Regrex from AI assistants (MCP)

`regrex mcp` runs a [Model Context Protocol](https://modelcontextprotocol.io) server on stdin and stdout. Assistants that support MCP, such as Claude Desktop, Claude Code, Cursor or VS Code, can then scan your files and fit formulas themselves, instead of guessing.

The server only reads the files it is asked about, keeps no state and never uses the network.

## Setup

Install the command-line tool first:

```sh
cargo install regrex-cli
```

**Claude Code**

```sh
claude mcp add regrex -- regrex mcp
```

**Claude Desktop, Cursor and other clients** that use a JSON configuration file (`claude_desktop_config.json`, `.cursor/mcp.json`, …):

```json
{
  "mcpServers": {
    "regrex": { "command": "regrex", "args": ["mcp"] }
  }
}
```

**VS Code** (`.vscode/mcp.json`):

```json
{
  "servers": {
    "regrex": { "type": "stdio", "command": "regrex", "args": ["mcp"] }
  }
}
```

Use the full path to `regrex` (for example `~/.cargo/bin/regrex`) if the client does not find it.

## Tools

| Tool | What it does | Required arguments |
| --- | --- | --- |
| `describe_table` | Tells what a file holds without searching: every column with its values and range, how the table is laid out (groups, the order of the rows, the role of each column) and the relations between text columns. About as fast as reading the file. | `path` |
| `scan_dataset` | Finds the columns of a file computed from other columns, with the rows that break each formula. `quick: true` searches a sample of rows first and confirms on all rows (several times faster on large files); `format: "agent"` returns the compact report for judging the results. | `path` |
| `fit_formula` | Finds candidate formulas for one column of a file. | `path`, `target` |
| `fit_values` | Finds candidate formulas for values given directly. | `inputs`, `target` |
| `check_formula` | Checks a formula you suspect against a column and lists the rows that do not follow it. | `path`, `target`, `formula` |
| `check_units` | Computes the units of a formula and checks them against an expected unit. | `formula`, `units` |

The tools that read a file also take `sheet` (a sheet or a database table), `ignore` (columns to leave out) and `with`, a second file whose rows are matched with those of the first on the columns both have, such as a line, a track and a chainage, so relations between the two files are found.

Every tool returns the JSON described in [json-output.md](json-output.md), both as text and as `structuredContent`. Invalid arguments, such as a missing file or an unknown column, come back as a tool result with `isError: true` so the assistant can correct its call.

## Example prompts

- "Which columns of `sales.xlsx` are calculated from other columns? Are there rows that don't follow the rule?"
- "I measured these fall times and distances: … Find the formula."
- "Is `0.5 * m * v^2` an energy if `m` is in kg and `v` in m/s?"
- "What is in `track.parquet`? Then find the formulas, quickly."
- "How do the columns of `design.csv` relate to the measurements in `survey.csv`?"

## Testing the server by hand

```sh
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"sh","version":"0"}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"scan_dataset","arguments":{"path":"examples/data/invoices.csv","ignore":["invoice"]}}}' \
  | regrex mcp
```

Or use the [MCP Inspector](https://github.com/modelcontextprotocol/inspector): `npx @modelcontextprotocol/inspector regrex mcp`.
