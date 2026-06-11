# goose-runtime-policy

`goose-runtime-policy` owns the prompt and runtime policy vocabulary shared by
Goose and embedding hosts.

Current ownership:

- prompt policy DTOs for system prompt extras and optional overrides
- runtime mode and runtime platform values
- prompt policy target and applier traits
- prompt section and prompt assembly helpers
- layered host policy rendering
- helpers for rendering prompt addenda without copying the base Goose prompt

Keep the concrete Goose system prompt, runtime-specific policy text, mode
selection, and provider behavior out of this crate. This crate defines how a
host describes policy; the owning runtime decides what policy to apply.
