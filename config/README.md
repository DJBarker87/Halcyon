# Halcyon Config Examples

The files under `config/examples/` are templates for Layer 5 launch ops.

Guidance:

- copy an example to a real config path outside source control when possible
- fill mainnet-specific RPC endpoints, keypairs, and oracle accounts
- keep launch secrets and API keys out of git

Tracked exception:

- `config/delta_keeper.json` is a secret-free devnet profile for local one-shot testing
- it uses the current devnet Helius URL shape with `api-key=REPLACE_ME`, so the file stays safe to commit while still matching deployment
- it includes informational underscore-prefixed fields for the compiled flagship and kernel IDs plus the required keeper role
- deployed keeper hosts should still render `/etc/halcyon/config/delta_keeper.json` from `ops/hetzner-deploy/env.example` so the real `HELIUS_DEVNET_RPC` stays in `/etc/halcyon/env`, not git

These files match the current keeper binaries and frontend runtime config keys.
