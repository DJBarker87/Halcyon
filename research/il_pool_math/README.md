IL pool-math reference implementation
=====================================

Moved out of `crates/halcyon_il_quote/src/pool/` on 2026-04-20 (L3 audit, finding L3-L2).

These files were never wired into `halcyon_il_quote::lib.rs`; they reference
a `halcyon_common` sub-surface (`fp`, `fees`, `constants`) that does not
exist in this repo. They are retained as reference material for a future
LP-path variant of IL Protection.

Not compiled. Not part of the workspace. Treat as documentation until the
LP-path scope reopens.
