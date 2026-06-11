# goose-tool-runtime

`goose-tool-runtime` owns the reusable Tool/MCP policy and dispatch preparation
surface for runtimes that want Goose tool execution with externally supplied
permissions.

Current ownership:

- runtime session and tool invocation DTOs
- workspace bindings for runtime-owned local workspaces
- per-session tool availability and approval policy
- approval decisions and dispatch authorization helpers
- explainable tool inventory visibility for available and blocked tools
- argument validation and prepared dispatch requests
- working-directory selection for bound workspaces

Keep concrete MCP registry implementations, actual tool execution, authorization
stores, workspace materialization, and approval presentation out of this crate.
This crate prepares and validates dispatch; the embedding runtime owns the
registry, permissions, and execution path.
