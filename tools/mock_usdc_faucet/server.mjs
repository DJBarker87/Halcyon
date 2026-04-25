import http from "node:http";
import fs from "node:fs";
import {
  Connection,
  Keypair,
  PublicKey,
  SYSVAR_RENT_PUBKEY,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import bs58 from "bs58";

const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const DEFAULT_AMOUNT_UI = "25000";
const DECIMALS = 6;

function env(name, fallback = "") {
  const value = process.env[name]?.trim();
  return value ? value : fallback;
}

function requiredEnv(name) {
  const value = env(name);
  if (!value) throw new Error(`Missing required env var ${name}`);
  return value;
}

function loadKeypair() {
  const json = env("MOCK_USDC_FAUCET_KEYPAIR_JSON");
  const path = env("MOCK_USDC_FAUCET_KEYPAIR_PATH");
  const base58 = env("MOCK_USDC_FAUCET_KEYPAIR_BASE58");

  if (json) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(json)));
  if (path) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(path, "utf8"))));
  if (base58) return Keypair.fromSecretKey(bs58.decode(base58));
  throw new Error(
    "Set MOCK_USDC_FAUCET_KEYPAIR_PATH, MOCK_USDC_FAUCET_KEYPAIR_JSON, or MOCK_USDC_FAUCET_KEYPAIR_BASE58",
  );
}

function parseAmountUi(input) {
  const raw = String(input ?? DEFAULT_AMOUNT_UI).trim();
  if (!/^\d+(\.\d{1,6})?$/.test(raw)) {
    throw new Error("Amount must be a decimal USDC value with at most 6 decimals");
  }
  const [whole, fraction = ""] = raw.split(".");
  return BigInt(whole) * 10n ** BigInt(DECIMALS) + BigInt(fraction.padEnd(DECIMALS, "0"));
}

function formatAmount(baseUnits) {
  const whole = baseUnits / 10n ** BigInt(DECIMALS);
  const fraction = (baseUnits % 10n ** BigInt(DECIMALS)).toString().padStart(DECIMALS, "0");
  const trimmed = fraction.replace(/0+$/, "");
  return trimmed ? `${whole}.${trimmed}` : whole.toString();
}

function getAssociatedTokenAddress(owner, mint) {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  )[0];
}

function createAssociatedTokenAccountInstruction(payer, ata, owner, mint) {
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: ata, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false },
    ],
    data: Buffer.alloc(0),
  });
}

function mintToInstruction(mint, destination, authority, amount) {
  const data = Buffer.alloc(9);
  data.writeUInt8(7, 0);
  data.writeBigUInt64LE(amount, 1);
  return new TransactionInstruction({
    programId: TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: mint, isSigner: false, isWritable: true },
      { pubkey: destination, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    data,
  });
}

function json(res, status, payload, origin) {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    "content-type": "application/json",
    "cache-control": "no-store",
    "access-control-allow-origin": origin,
    "access-control-allow-methods": "GET,POST,OPTIONS",
    "access-control-allow-headers": "content-type",
  });
  res.end(body);
}

async function readBody(req) {
  let body = "";
  for await (const chunk of req) {
    body += chunk;
    if (body.length > 2048) throw new Error("Request body is too large");
  }
  return body ? JSON.parse(body) : {};
}

function requestIp(req) {
  const forwarded = req.headers["x-forwarded-for"];
  if (typeof forwarded === "string" && forwarded.trim()) {
    return forwarded.split(",")[0].trim();
  }
  return req.socket.remoteAddress ?? "unknown";
}

const rpcUrl = env("RPC_URL", env("HELIUS_DEVNET_RPC", "https://api.devnet.solana.com"));
const connection = new Connection(rpcUrl, "confirmed");
const mint = new PublicKey(requiredEnv("MOCK_USDC_MINT"));
const faucet = loadKeypair();
const amount = parseAmountUi(env("MOCK_USDC_FAUCET_AMOUNT", DEFAULT_AMOUNT_UI));
const maxAmount = parseAmountUi(env("MOCK_USDC_FAUCET_MAX_AMOUNT", DEFAULT_AMOUNT_UI));
const cooldownMs = Number.parseInt(env("MOCK_USDC_FAUCET_COOLDOWN_MS", "3600000"), 10);
const port = Number.parseInt(env("PORT", "8787"), 10);
const allowedOrigins = env("MOCK_USDC_FAUCET_ALLOWED_ORIGINS", "*")
  .split(",")
  .map((entry) => entry.trim())
  .filter(Boolean);
const claims = new Map();

function allowedOrigin(req) {
  const origin = req.headers.origin;
  if (allowedOrigins.includes("*")) return "*";
  if (typeof origin === "string" && allowedOrigins.includes(origin)) return origin;
  return allowedOrigins[0] ?? "*";
}

async function handleAirdrop(req, res) {
  const origin = allowedOrigin(req);
  const body = await readBody(req);
  const wallet = new PublicKey(String(body.wallet ?? ""));
  if (!PublicKey.isOnCurve(wallet.toBytes())) {
    throw new Error("Wallet must be a normal wallet public key");
  }

  const requestedAmount = body.amount ? parseAmountUi(body.amount) : amount;
  if (requestedAmount <= 0n || requestedAmount > maxAmount) {
    throw new Error(`Amount must be between 0 and ${formatAmount(maxAmount)} mockUSDC`);
  }

  const now = Date.now();
  const key = `${wallet.toBase58()}:${requestIp(req)}`;
  const previous = claims.get(key) ?? 0;
  if (previous && now - previous < cooldownMs) {
    const retryAfterMs = cooldownMs - (now - previous);
    json(
      res,
      429,
      {
        error: "cooldown",
        retryAfterMs,
      },
      origin,
    );
    return;
  }

  const ata = getAssociatedTokenAddress(wallet, mint);
  const ataInfo = await connection.getAccountInfo(ata, "confirmed");
  const tx = new Transaction();
  if (!ataInfo) {
    tx.add(createAssociatedTokenAccountInstruction(faucet.publicKey, ata, wallet, mint));
  }
  tx.add(mintToInstruction(mint, ata, faucet.publicKey, requestedAmount));

  const signature = await sendAndConfirmTransaction(connection, tx, [faucet], {
    commitment: "confirmed",
  });
  claims.set(key, now);

  json(
    res,
    200,
    {
      ok: true,
      signature,
      mint: mint.toBase58(),
      wallet: wallet.toBase58(),
      tokenAccount: ata.toBase58(),
      amount: formatAmount(requestedAmount),
      explorerUrl: `https://explorer.solana.com/tx/${signature}?cluster=devnet`,
    },
    origin,
  );
}

const server = http.createServer(async (req, res) => {
  const origin = allowedOrigin(req);
  try {
    if (req.method === "OPTIONS") {
      json(res, 204, {}, origin);
      return;
    }
    if (req.method === "GET" && req.url === "/health") {
      json(
        res,
        200,
        {
          ok: true,
          cluster: "devnet",
          mint: mint.toBase58(),
          faucet: faucet.publicKey.toBase58(),
          amount: formatAmount(amount),
          maxAmount: formatAmount(maxAmount),
          cooldownMs,
        },
        origin,
      );
      return;
    }
    if (req.method === "POST" && req.url === "/airdrop") {
      await handleAirdrop(req, res);
      return;
    }
    json(res, 404, { error: "not_found" }, origin);
  } catch (error) {
    json(res, 400, { error: error instanceof Error ? error.message : String(error) }, origin);
  }
});

server.listen(port, () => {
  console.log(
    `mock-USDC faucet listening on :${port} mint=${mint.toBase58()} faucet=${faucet.publicKey.toBase58()}`,
  );
});
