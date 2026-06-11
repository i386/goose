# goose-tool-runtime

`goose-tool-runtime` owns the reusable Tool/MCP policy and dispatch preparation
surface for hosts that want Goose tool execution with host-controlled
permissions.

Current ownership:

- runtime session and tool invocation DTOs
- workspace bindings for host-owned local workspaces
- per-session tool availability and approval policy
- host approval decisions and dispatch authorization helpers
- explainable tool inventory visibility for available and blocked tools
- argument validation and prepared dispatch requests
- working-directory selection for bound workspaces

Keep concrete MCP registry implementations, actual tool execution, user
authorization stores, file synchronization, and UI approval flows out of this
crate. This crate prepares and validates dispatch; the embedding runtime owns
the registry, permissions, and execution path.
