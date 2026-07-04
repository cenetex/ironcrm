# White Warren MCP Server

White Warren can run as a local stdio MCP server so Claude Desktop, Codex, and other MCP clients can inspect and update the same local CRM database used by the desktop app.

## Quick Start

From this repo:

```sh
cargo run -- mcp
```

For client configuration, use the wrapper script so the MCP client does not need to know the repo working directory:

```sh
/Users/ratimics/develop/ironcrm/scripts/ironcrm-mcp.sh
```

By default the MCP server opens:

```text
~/Library/Application Support/White Warren/white-warren.db
```

Override it with `LOCAL_DB_PATH`, `WHITE_WARREN_DB`, or the legacy `IRONCRM_DB`.

## Claude Desktop

Open Claude Desktop settings, go to Developer settings, edit the MCP config, and add:

```json
{
  "mcpServers": {
    "ironcrm": {
      "command": "/bin/bash",
      "args": ["/Users/ratimics/develop/ironcrm/scripts/ironcrm-mcp.sh"],
      "env": {
        "LOCAL_DB_PATH": "/Users/ratimics/Library/Application Support/White Warren/white-warren.db"
      }
    }
  }
}
```

Restart Claude Desktop or reload MCP configuration after saving.

## Claude Desktop MCPB Package

To build a one-click Claude Desktop package for the current macOS release binary:

```sh
./scripts/package-mcpb.sh
```

The generated `.mcpb` file is written under `dist/`. The package bundles the compiled `ironcrm` binary and uses the default desktop database unless the user chooses another SQLite file during installation.

## Codex

Add this to `~/.codex/config.toml` or to a project-scoped `.codex/config.toml`:

```toml
[mcp_servers.ironcrm]
command = "/bin/bash"
args = ["/Users/ratimics/develop/ironcrm/scripts/ironcrm-mcp.sh"]
enabled = true

[mcp_servers.ironcrm.env]
LOCAL_DB_PATH = "/Users/ratimics/Library/Application Support/White Warren/white-warren.db"
```

Restart Codex, or reload MCP servers from the app settings.

## Tools

- `ironcrm_workspace_summary` - module names and record counts
- `ironcrm_list_modules` - module schemas and fields
- `ironcrm_list_records` - records for one module
- `ironcrm_search_records` - search records and note activity
- `ironcrm_get_record` - record detail with relationships and activities
- `ironcrm_create_record` - create a local record
- `ironcrm_update_record` - update local record fields
- `ironcrm_add_note` - add an activity note
- `ironcrm_link_records` - create a relationship between two records
- `ironcrm_detect_suitecrm` - detect local SuiteCRM config
- `ironcrm_export_workspace` - export a complete workspace bundle

## Resources

- `ironcrm://workspace/summary`
- `ironcrm://modules`

## Safety

This MCP server is local-only and stdio-only. It does not open a network port. Write tools mutate the selected local SQLite database, so point `LOCAL_DB_PATH` at a test database if you want a read-only-ish playground.
