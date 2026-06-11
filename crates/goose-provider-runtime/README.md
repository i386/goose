# goose-provider-runtime

`goose-provider-runtime` owns the reusable provider-facing runtime surface for
hosts that need Goose provider behavior without pulling in the full Goose agent
or session runtime.

Current ownership:

- provider factory, creator, inventory, registry, and entry traits
- provider runtime config, model spec, model hints, and metadata DTOs
- model config target and snapshot traits for host-owned model configuration
- streaming message helpers and provider usage accounting
- retry policy defaults and retry configuration helpers
- normalized provider failures, provider errors, and Google error code mapping
- permission routing metadata used by tool-aware provider calls

Keep concrete agent orchestration, prompt management, session persistence,
workspace binding, and UI behavior out of this crate. Provider implementations
can adapt to this surface, but this crate should stay focused on reusable
provider contracts and dependency-light provider support code.
