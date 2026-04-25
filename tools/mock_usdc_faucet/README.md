# Halcyon Mock USDC Faucet

Static frontend builds cannot hold a mint-authority key. This service keeps the
mock-USDC mint authority server-side and exposes a tiny CORS API:

- `GET /health`
- `POST /airdrop` with `{ "wallet": "<pubkey>" }`

Devnet bring-up:

```bash
cd tools/mock_usdc_faucet
npm ci
RPC_URL=https://api.devnet.solana.com \
PAYER_KEYPAIR_PATH=~/.config/solana/id.json \
npm run create-mint
```

Then initialize Halcyon's kernel custody accounts for the new mint:

```bash
USDC_MINT=<printed mint> \
halcyon --rpc https://api.devnet.solana.com --keypair ~/.config/solana/id.json \
  init-payment-mint
```

Run the faucet:

```bash
RPC_URL=https://api.devnet.solana.com \
MOCK_USDC_MINT=<printed mint> \
MOCK_USDC_FAUCET_KEYPAIR_PATH=ops/devnet_keys/mock-usdc-faucet.json \
MOCK_USDC_FAUCET_ALLOWED_ORIGINS=https://halcyonprotocol.xyz,https://www.halcyonprotocol.xyz \
npm start
```

Set the frontend build env:

```bash
NEXT_PUBLIC_USDC_MINT_DEVNET=<printed mint>
NEXT_PUBLIC_MOCK_USDC_FAUCET_URL=/faucet-api
```
