# rustdoc-mcp

[![codecov](https://codecov.io/gh/jbr/rustdoc-mcp/graph/badge.svg?token=FDpsPBK9zl)](https://codecov.io/gh/jbr/rustdoc-mcp)
[![ci][ci-badge]][ci]
[![crates.io version badge][version-badge]][crate]

[ci]: https://github.com/jbr/rustdoc-mcp/actions?query=workflow%3ACI
[ci-badge]: https://github.com/jbr/rustdoc-mcp/workflows/CI/badge.svg
[version-badge]: https://img.shields.io/crates/v/rustdoc-mcp.svg?style=flat-square
[crate]: https://crates.io/crates/rustdoc-mcp



A Model Context Protocol server for rust documentation

Note: This crate requires `nightly` to be installed, since it uses unstable rustdoc json output format.

Additionally, if you want to allow your MCP users to reference `std`, `core`, `alloc`, `proc_macro`,
and `test`, you'll need to `rustup component add rust-docs-json`. This is optional.

## Tools
```
  set-working-directory  Set the working context path for a session
  get-item               Get detailed information about a specific item or list items in a module/crate
  list-crates            List available crates in the workspace, including dependencies
  search                 Search for items within a specific crate
```

## Installation

```bash
$ cargo install rustdoc-mcp
```

## Usage with Claude Desktop or gemini-cli

Add this to your MCP configuration JSON file:

```json
{
  "mcpServers": {
    "rustdocs": {
      "command": "/path/to/rustdoc-mcp/rustdoc-mcp",
      "args": ["serve"]
    }
  }
}
```


## License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

---

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
