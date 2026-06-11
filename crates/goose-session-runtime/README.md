# goose-session-runtime

`goose-session-runtime` owns the reusable session and history persistence
contract for runtimes that map external session identity onto Goose sessions.

Current ownership:

- external session and task identifiers
- links between external sessions and Goose runtime session ids
- workspace references for local paths, remote URIs, owners, and mount names
- runtime session records with metadata and local workspace binding
- runtime history entries and conversion to and from Goose messages
- session link, runtime session catalog, and history persistence traits

Keep concrete storage implementations, authorization policy, workspace
materialization, and interaction surfaces out of this crate. This crate defines
the persistence boundary; the host supplies the backing store and workspace
binding policy.
