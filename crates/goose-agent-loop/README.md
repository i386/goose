# goose-agent-loop

`goose-agent-loop` owns the reusable agent loop contract shared by Goose and
host runtimes that embed Goose behavior.

Current ownership:

- loop request, options, session, and retry specifications
- turn-limit and cancellation control primitives
- source-runtime and runtime traits for host-owned loop implementations
- normalized loop events for text, tool calls, tool results, MCP notifications,
  action requests, and history replacement
- lifecycle event wrapping for run start and completion
- conversion from Goose conversation messages into loop events

Keep concrete provider creation, concrete tool dispatch, session storage, CLI
rendering, and product-specific prompt policy out of this crate. The loop crate
coordinates those contracts; the owning runtime supplies their implementations.
