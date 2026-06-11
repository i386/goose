# goose-agent-core

`goose-agent-core` is the small aggregate crate for embedders that want the
reusable Goose runtime contracts without depending on the full Goose crate.

Current ownership:

- re-exports for provider runtime contracts
- re-exports for agent loop contracts
- re-exports for prompt/runtime policy contracts
- re-exports for session runtime contracts
- re-exports for tool runtime contracts

Keep concrete Goose agent orchestration, provider implementations, persistence
implementations, and interaction surfaces out of this crate. Those belong in the
owning runtime or the specific runtime contract crates.
