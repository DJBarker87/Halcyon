import http from "node:http";
import fs from "node:fs";
import { spawn } from "node:child_process";
import {
  AddressLookupTableAccount,
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SYSVAR_RENT_PUBKEY,
  SystemProgram,
  SystemInstruction,
  Transaction,
  TransactionInstruction,
  VersionedTransaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import bs58 from "bs58";

const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const COMPUTE_BUDGET_PROGRAM_ID = new PublicKey("ComputeBudget111111111111111111111111111111");
const MEMO_PROGRAM_ID = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
const DEFAULT_AMOUNT_UI = "25000";
const DECIMALS = 6;
const DEFAULT_DEMO_WALLET_MAX_NOTIONAL_UI = "25000";
const DEFAULT_DEMO_WALLET_TARGET_USDC_UI = "1000000";

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
  return loadKeypairFromEnv("MOCK_USDC_FAUCET");
}

function loadKeypairFromEnv(prefix) {
  const json = env(`${prefix}_KEYPAIR_JSON`);
  const path = env(`${prefix}_KEYPAIR_PATH`);
  const base58 = env(`${prefix}_KEYPAIR_BASE58`);

  if (json) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(json)));
  if (path) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(path, "utf8"))));
  if (base58) return Keypair.fromSecretKey(bs58.decode(base58));
  throw new Error(
    `Set ${prefix}_KEYPAIR_PATH, ${prefix}_KEYPAIR_JSON, or ${prefix}_KEYPAIR_BASE58`,
  );
}

function loadOptionalKeypairFromEnv(prefix) {
  const json = env(`${prefix}_KEYPAIR_JSON`);
  const path = env(`${prefix}_KEYPAIR_PATH`);
  const base58 = env(`${prefix}_KEYPAIR_BASE58`);

  if (json) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(json)));
  if (path) return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(path, "utf8"))));
  if (base58) return Keypair.fromSecretKey(bs58.decode(base58));
  return null;
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

function parseSolUi(input, fallback) {
  const raw = String(input ?? fallback).trim();
  if (!/^\d+(\.\d{1,9})?$/.test(raw)) {
    throw new Error("SOL amount must be a decimal value with at most 9 decimals");
  }
  const [whole, fraction = ""] = raw.split(".");
  return Number.parseInt(whole, 10) * LAMPORTS_PER_SOL + Number.parseInt(fraction.padEnd(9, "0"), 10);
}

function formatSol(lamports) {
  return (lamports / LAMPORTS_PER_SOL).toFixed(4).replace(/\.?0+$/, "");
}

function safeErrorMessage(error) {
  const raw = error instanceof Error ? error.message : String(error);
  return raw
    .replace(/(api-key=)[^&\s]+/gi, "$1[redacted]")
    .replace(/(Authorization:\s*Bearer\s+)[^\s]+/gi, "$1[redacted]")
    .replace(/(HELIUS_DEVNET_RPC=)[^\s]+/gi, "$1[redacted]");
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

async function readBody(req, maxBytes = 2048) {
  let body = "";
  for await (const chunk of req) {
    body += chunk;
    if (body.length > maxBytes) throw new Error("Request body is too large");
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
const demoWallet = loadOptionalKeypairFromEnv("DEMO_WALLET");
const demoWalletKeypairPath = env("DEMO_WALLET_KEYPAIR_PATH");
const demoWalletCliPath = env("DEMO_WALLET_CLI_PATH", "/opt/halcyon/bin/halcyon");
const demoWalletMaxNotional = parseAmountUi(
  env("DEMO_WALLET_MAX_NOTIONAL", DEFAULT_DEMO_WALLET_MAX_NOTIONAL_UI),
);
const demoWalletTargetUsdc = parseAmountUi(
  env("DEMO_WALLET_TARGET_USDC", DEFAULT_DEMO_WALLET_TARGET_USDC_UI),
);
const demoWalletMinSolLamports = parseSolUi(env("DEMO_WALLET_MIN_SOL", "0.25"), "0.25");
const demoWalletTargetSolLamports = parseSolUi(env("DEMO_WALLET_TARGET_SOL", "1"), "1");
const demoWalletCommandTimeoutMs = Number.parseInt(env("DEMO_WALLET_COMMAND_TIMEOUT_MS", "120000"), 10);
const demoWalletMaxCreateAccountLamports = parseSolUi(
  env("DEMO_WALLET_MAX_CREATE_ACCOUNT_SOL", "0.35"),
  "0.35",
);
const cooldownMs = Number.parseInt(env("MOCK_USDC_FAUCET_COOLDOWN_MS", "3600000"), 10);
const port = Number.parseInt(env("PORT", "8787"), 10);
const allowedOrigins = env("MOCK_USDC_FAUCET_ALLOWED_ORIGINS", "*")
  .split(",")
  .map((entry) => entry.trim())
  .filter(Boolean);
const demoWalletAllowedOrigins = env(
  "DEMO_WALLET_ALLOWED_ORIGINS",
  env("MOCK_USDC_FAUCET_ALLOWED_ORIGINS", "*"),
)
  .split(",")
  .map((entry) => entry.trim())
  .filter(Boolean);
const claims = new Map();
let demoWalletQueue = Promise.resolve();
const allowedHalcyonProgramIds = [
  env("KERNEL_PROGRAM_ID"),
  env("LENDING_CONSUMER_PROGRAM_ID"),
  env("FLAGSHIP_PROGRAM_ID"),
  env("IL_PROGRAM_ID"),
  env("SOL_PROGRAM_ID"),
]
  .filter(Boolean)
  .map((value) => new PublicKey(value));
const allowedProgramIds = [
  SystemProgram.programId,
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  COMPUTE_BUDGET_PROGRAM_ID,
  MEMO_PROGRAM_ID,
  ...allowedHalcyonProgramIds,
];

function allowedOrigin(req) {
  const origin = req.headers.origin;
  if (allowedOrigins.includes("*")) return "*";
  if (typeof origin === "string" && allowedOrigins.includes(origin)) return origin;
  return allowedOrigins[0] ?? "*";
}

function isAllowedRequestOrigin(req, origins) {
  if (origins.includes("*")) return true;
  const origin = req.headers.origin;
  return typeof origin === "string" && origins.includes(origin);
}

function assertDemoWalletRequestOrigin(req) {
  if (!isAllowedRequestOrigin(req, demoWalletAllowedOrigins)) {
    throw new Error("Demo Wallet request origin is not allowed");
  }
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

function demoWalletRequired() {
  if (!demoWallet) {
    throw new Error("Demo wallet is not configured");
  }
  if (!demoWalletKeypairPath) {
    throw new Error("DEMO_WALLET_KEYPAIR_PATH is required for demo wallet actions");
  }
  return demoWallet;
}

async function tokenBalanceBaseUnits(owner) {
  const ata = getAssociatedTokenAddress(owner, mint);
  const info = await connection.getAccountInfo(ata, "confirmed");
  if (!info) return { ata, amount: 0n };
  const balance = await connection.getTokenAccountBalance(ata, "confirmed");
  return { ata, amount: BigInt(balance.value.amount) };
}

async function ensureMockUsdc(owner, targetAmount) {
  const { ata, amount: currentAmount } = await tokenBalanceBaseUnits(owner);
  if (currentAmount >= targetAmount) {
    return { signature: null, tokenAccount: ata, previousAmount: currentAmount, mintedAmount: 0n };
  }

  const ataInfo = await connection.getAccountInfo(ata, "confirmed");
  const tx = new Transaction();
  if (!ataInfo) {
    tx.add(createAssociatedTokenAccountInstruction(faucet.publicKey, ata, owner, mint));
  }
  const mintedAmount = targetAmount - currentAmount;
  tx.add(mintToInstruction(mint, ata, faucet.publicKey, mintedAmount));
  const signature = await sendAndConfirmTransaction(connection, tx, [faucet], {
    commitment: "confirmed",
  });
  return { signature, tokenAccount: ata, previousAmount: currentAmount, mintedAmount };
}

async function ensureDemoSol(owner) {
  const balance = await connection.getBalance(owner, "confirmed");
  if (balance >= demoWalletMinSolLamports) {
    return { signature: null, previousLamports: balance, transferredLamports: 0 };
  }

  const requiredLamports = Math.max(0, demoWalletTargetSolLamports - balance);
  const faucetBalance = await connection.getBalance(faucet.publicKey, "confirmed");
  if (faucetBalance > requiredLamports + 50_000_000) {
    const tx = new Transaction().add(
      SystemProgram.transfer({
        fromPubkey: faucet.publicKey,
        toPubkey: owner,
        lamports: requiredLamports,
      }),
    );
    const signature = await sendAndConfirmTransaction(connection, tx, [faucet], {
      commitment: "confirmed",
    });
    return { signature, previousLamports: balance, transferredLamports: requiredLamports };
  }

  const signature = await connection.requestAirdrop(owner, requiredLamports);
  await connection.confirmTransaction(signature, "confirmed");
  return { signature, previousLamports: balance, transferredLamports: requiredLamports };
}

async function ensureDemoWalletFunded() {
  const wallet = demoWalletRequired();
  const [solTopUp, usdcTopUp] = await Promise.all([
    ensureDemoSol(wallet.publicKey),
    ensureMockUsdc(wallet.publicKey, demoWalletTargetUsdc),
  ]);
  return { solTopUp, usdcTopUp };
}

function normalizeProduct(input) {
  const value = String(input ?? "").trim().toLowerCase();
  if (["sol", "sol-autocall", "solautocall"].includes(value)) return "solAutocall";
  if (["il", "il-protection", "ilprotection"].includes(value)) return "ilProtection";
  if (["flagship", "flagship-autocall", "flagshipautocall"].includes(value)) return "flagship";
  throw new Error("Product must be one of flagship, solAutocall, or ilProtection");
}

function requiredFeed(name) {
  return requiredEnv(name);
}

function cliArgsForIssue(product, amountBaseUnits, body) {
  const common = [
    "--rpc",
    rpcUrl,
    "--keypair",
    demoWalletKeypairPath,
  ];
  const toleranceArgs = [
    "--usdc-mint",
    mint.toBase58(),
    "--premium-slippage-bps",
    String(Math.max(0, Number(body.premiumSlippageBps ?? 50) || 0)),
    "--max-liability-floor-bps",
    String(Math.max(0, Number(body.maxLiabilityFloorBps ?? 50) || 0)),
    "--entry-drift-bps",
    String(Math.max(0, Number(body.entryDriftBps ?? 25) || 0)),
    "--max-quote-slot-delta",
    String(Math.max(0, Number(body.maxQuoteSlotDelta ?? 150) || 0)),
    "--max-expiry-delta-secs",
    String(Math.max(0, Number(body.maxExpiryDeltaSecs ?? 60) || 0)),
  ];

  if (product === "flagship") {
    return [
      ...common,
      "buy-flagship",
      String(amountBaseUnits),
      ...toleranceArgs,
      "--pyth-spy",
      requiredFeed("PYTH_SPY"),
      "--pyth-qqq",
      requiredFeed("PYTH_QQQ"),
      "--pyth-iwm",
      requiredFeed("PYTH_IWM"),
    ];
  }
  if (product === "ilProtection") {
    return [
      ...common,
      "buy-il",
      String(amountBaseUnits),
      ...toleranceArgs,
      "--pyth-sol",
      requiredFeed("PYTH_SOL"),
      "--pyth-usdc",
      requiredFeed("PYTH_USDC"),
    ];
  }
  return [
    ...common,
    "buy",
    String(amountBaseUnits),
    ...toleranceArgs,
    "--pyth-sol",
    requiredFeed("PYTH_SOL"),
  ];
}

function runDemoWalletCli(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(demoWalletCliPath, args, {
      env: { ...process.env, NO_DNA: "1" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new Error("Demo wallet command timed out"));
    }, demoWalletCommandTimeoutMs);

    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString("utf8");
      if (stdout.length > 100_000) stdout = stdout.slice(-100_000);
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString("utf8");
      if (stderr.length > 100_000) stderr = stderr.slice(-100_000);
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      if (code === 0) {
        resolve({ stdout, stderr });
        return;
      }
      reject(new Error((stderr || stdout || `Demo wallet command failed with code ${code}`).trim()));
    });
  });
}

function parseCliFields(stdout) {
  const fields = {};
  for (const rawLine of stdout.split(/\r?\n/)) {
    const line = rawLine.trim();
    const match = line.match(/(?:^|[\s:])([A-Za-z0-9_]+)=([^\s]+)/);
    if (match) fields[match[1]] = match[2];
  }
  return fields;
}

async function handleDemoWalletStatus(req, res) {
  const origin = allowedOrigin(req);
  if (!demoWallet) {
    json(res, 200, { ok: true, enabled: false, reason: "not_configured" }, origin);
    return;
  }
  let solLamports = null;
  let tokenAccount = null;
  let usdcAmount = null;
  let balanceError = null;
  try {
    solLamports = await connection.getBalance(demoWallet.publicKey, "confirmed");
    const balance = await tokenBalanceBaseUnits(demoWallet.publicKey);
    tokenAccount = balance.ata;
    usdcAmount = balance.amount;
  } catch (error) {
    balanceError = safeErrorMessage(error);
  }
  json(
    res,
    200,
    {
      ok: true,
      enabled: true,
      cluster: "devnet",
      wallet: demoWallet.publicKey.toBase58(),
      mint: mint.toBase58(),
      tokenAccount: tokenAccount?.toBase58() ?? null,
      sol: solLamports === null ? null : formatSol(solLamports),
      solLamports,
      mockUsdc: usdcAmount === null ? null : formatAmount(usdcAmount),
      mockUsdcBaseUnits: usdcAmount?.toString() ?? null,
      maxNotional: formatAmount(demoWalletMaxNotional),
      balanceError,
    },
    origin,
  );
}

async function handleDemoWalletTopUp(req, res) {
  assertDemoWalletRequestOrigin(req);
  const origin = allowedOrigin(req);
  const topUp = await enqueueDemoWalletAction(() => ensureDemoWalletFunded());
  json(
    res,
    200,
    {
      ok: true,
      wallet: demoWallet.publicKey.toBase58(),
      solSignature: topUp.solTopUp.signature,
      usdcSignature: topUp.usdcTopUp.signature,
      tokenAccount: topUp.usdcTopUp.tokenAccount.toBase58(),
      mintedMockUsdc: formatAmount(topUp.usdcTopUp.mintedAmount),
      transferredSol: formatSol(topUp.solTopUp.transferredLamports),
    },
    origin,
  );
}

async function enqueueDemoWalletAction(action) {
  const run = demoWalletQueue.then(action, action);
  demoWalletQueue = run.catch(() => {});
  return run;
}

function isAllowedProgram(programId) {
  return allowedProgramIds.some((allowed) => allowed.equals(programId));
}

function isHalcyonProgram(programId) {
  return allowedHalcyonProgramIds.some((allowed) => allowed.equals(programId));
}

function publicKeysEqual(left, right) {
  return left?.equals?.(right) ?? false;
}

function deserializeTransaction(serializedBase64) {
  const bytes = Buffer.from(String(serializedBase64 ?? ""), "base64");
  try {
    return { kind: "versioned", transaction: VersionedTransaction.deserialize(bytes) };
  } catch {
    return { kind: "legacy", transaction: Transaction.from(bytes) };
  }
}

async function addressLookupTablesFor(message) {
  if (!("addressTableLookups" in message) || message.addressTableLookups.length === 0) return [];
  const accounts = await connection.getMultipleAccountsInfo(
    message.addressTableLookups.map((lookup) => lookup.accountKey),
    "confirmed",
  );
  return message.addressTableLookups.map((lookup, index) => {
    const info = accounts[index];
    if (!info?.data) {
      throw new Error(`Address lookup table not found: ${lookup.accountKey.toBase58()}`);
    }
    return new AddressLookupTableAccount({
      key: lookup.accountKey,
      state: AddressLookupTableAccount.deserialize(info.data),
    });
  });
}

async function versionedInstructions(transaction) {
  const lookupTables = await addressLookupTablesFor(transaction.message);
  const accountKeys = transaction.message.getAccountKeys({
    addressLookupTableAccounts: lookupTables,
  });
  return transaction.message.compiledInstructions.map((instruction) => {
    const programId = accountKeys.get(instruction.programIdIndex);
    const keys = instruction.accountKeyIndexes.map((index) => ({
      pubkey: accountKeys.get(index),
      isSigner: index < transaction.message.header.numRequiredSignatures,
      isWritable: transaction.message.isAccountWritable(index),
    }));
    return new TransactionInstruction({
      programId,
      keys,
      data: Buffer.from(instruction.data),
    });
  });
}

function assertSystemInstructionAllowed(instruction, signerPubkey) {
  const type = SystemInstruction.decodeInstructionType(instruction);
  if (type === "Transfer") {
    const decoded = SystemInstruction.decodeTransfer(instruction);
    if (!decoded.fromPubkey.equals(signerPubkey)) {
      throw new Error("Demo Wallet only allows SystemProgram transfers from itself");
    }
    if (decoded.lamports !== 1) {
      throw new Error("Demo Wallet only allows the 1-lamport demo lending marker transfer");
    }
    return;
  }

  if (type !== "Create") {
    throw new Error(`Demo Wallet rejects SystemProgram ${type}`);
  }
  const decoded = SystemInstruction.decodeCreateAccount(instruction);
  if (!decoded.fromPubkey.equals(signerPubkey)) {
    throw new Error("Demo Wallet only allows create-account funding from itself");
  }
  if (decoded.lamports > demoWalletMaxCreateAccountLamports) {
    throw new Error("Demo Wallet create-account rent exceeds configured limit");
  }
  if (!isHalcyonProgram(decoded.programId)) {
    throw new Error("Demo Wallet only creates accounts owned by Halcyon programs");
  }
}

function assertAssociatedTokenInstructionAllowed(instruction, signerPubkey) {
  const payer = instruction.keys[0]?.pubkey;
  const owner = instruction.keys[2]?.pubkey;
  if (!publicKeysEqual(payer, signerPubkey) || !publicKeysEqual(owner, signerPubkey)) {
    throw new Error("Demo Wallet only creates its own associated token accounts");
  }
}

async function assertTransactionAllowed(kind, transaction, signerPubkey) {
  const instructions = kind === "versioned" ? await versionedInstructions(transaction) : transaction.instructions;
  if (instructions.length === 0) throw new Error("Transaction has no instructions");
  if (!instructions.some((instruction) => isHalcyonProgram(instruction.programId))) {
    throw new Error("Demo Wallet only signs Halcyon product transactions");
  }

  let tinySystemTransfers = 0;
  for (const instruction of instructions) {
    if (!isAllowedProgram(instruction.programId)) {
      throw new Error(`Demo Wallet rejects program ${instruction.programId.toBase58()}`);
    }
    if (instruction.programId.equals(SystemProgram.programId)) {
      if (SystemInstruction.decodeInstructionType(instruction) === "Transfer") {
        tinySystemTransfers += 1;
      }
      assertSystemInstructionAllowed(instruction, signerPubkey);
    } else if (instruction.programId.equals(ASSOCIATED_TOKEN_PROGRAM_ID)) {
      assertAssociatedTokenInstructionAllowed(instruction, signerPubkey);
    } else if (instruction.programId.equals(TOKEN_PROGRAM_ID)) {
      throw new Error("Demo Wallet rejects direct SPL-token instructions");
    }
  }
  if (tinySystemTransfers > 2) {
    throw new Error("Demo Wallet rejects transactions with more than two demo marker transfers");
  }
}

async function signAndSendDemoWalletTransaction(serializedBase64) {
  const wallet = demoWalletRequired();
  const { kind, transaction } = deserializeTransaction(serializedBase64);
  if (kind === "versioned") {
    const staticKeys = transaction.message.staticAccountKeys;
    if (!staticKeys[0]?.equals(wallet.publicKey)) {
      throw new Error("Demo Wallet must be the transaction fee payer");
    }
    const requiredSigners = staticKeys.slice(0, transaction.message.header.numRequiredSignatures);
    if (!requiredSigners.some((key) => key.equals(wallet.publicKey))) {
      throw new Error("Demo Wallet is not a required signer");
    }
    await assertTransactionAllowed(kind, transaction, wallet.publicKey);
    transaction.sign([wallet]);
    const signature = await connection.sendRawTransaction(transaction.serialize(), {
      maxRetries: 5,
      preflightCommitment: "confirmed",
    });
    await connection.confirmTransaction(signature, "confirmed");
    return signature;
  }

  if (!transaction.feePayer?.equals(wallet.publicKey)) {
    throw new Error("Demo Wallet must be the transaction fee payer");
  }
  await assertTransactionAllowed(kind, transaction, wallet.publicKey);
  transaction.partialSign(wallet);
  const signature = await connection.sendRawTransaction(
    transaction.serialize({ requireAllSignatures: true, verifySignatures: true }),
    {
      maxRetries: 5,
      preflightCommitment: "confirmed",
    },
  );
  await connection.confirmTransaction(signature, "confirmed");
  return signature;
}

async function handleDemoWalletIssue(req, res) {
  assertDemoWalletRequestOrigin(req);
  const origin = allowedOrigin(req);
  const body = await readBody(req);
  const product = normalizeProduct(body.product);
  const amountBaseUnits = body.amountBaseUnits
    ? BigInt(String(body.amountBaseUnits))
    : parseAmountUi(body.amount ?? DEFAULT_AMOUNT_UI);
  if (amountBaseUnits <= 0n || amountBaseUnits > demoWalletMaxNotional) {
    throw new Error(`Amount must be between 0 and ${formatAmount(demoWalletMaxNotional)} mockUSDC`);
  }

  const result = await enqueueDemoWalletAction(async () => {
    await ensureDemoWalletFunded();
    const args = cliArgsForIssue(product, amountBaseUnits, body);
    const { stdout, stderr } = await runDemoWalletCli(args);
    const fields = parseCliFields(stdout);
    const signature = fields.signature;
    if (!signature) {
      throw new Error(`Demo wallet issue succeeded but no signature was printed\n${stdout}`);
    }
    return { stdout, stderr, fields, signature };
  });

  json(
    res,
    200,
    {
      ok: true,
      product,
      wallet: demoWallet.publicKey.toBase58(),
      signature: result.signature,
      explorerUrl: `https://explorer.solana.com/tx/${result.signature}?cluster=devnet`,
      policy: result.fields.policy,
      policyId: result.fields.policy_id,
      productTerms: result.fields.product_terms,
      amount: formatAmount(amountBaseUnits),
      fields: result.fields,
    },
    origin,
  );
}

async function handleDemoWalletSendTransaction(req, res) {
  assertDemoWalletRequestOrigin(req);
  const origin = allowedOrigin(req);
  const body = await readBody(req, 300_000);
  const signature = await enqueueDemoWalletAction(async () => {
    await ensureDemoWalletFunded();
    return signAndSendDemoWalletTransaction(body.transaction);
  });
  json(
    res,
    200,
    {
      ok: true,
      wallet: demoWallet.publicKey.toBase58(),
      signature,
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
          demoWalletEnabled: Boolean(demoWallet),
          demoWallet: demoWallet?.publicKey.toBase58() ?? null,
        },
        origin,
      );
      return;
    }
    if (req.method === "GET" && req.url === "/demo-wallet/status") {
      await handleDemoWalletStatus(req, res);
      return;
    }
    if (req.method === "POST" && req.url === "/demo-wallet/top-up") {
      await handleDemoWalletTopUp(req, res);
      return;
    }
    if (req.method === "POST" && req.url === "/demo-wallet/issue") {
      await handleDemoWalletIssue(req, res);
      return;
    }
    if (req.method === "POST" && req.url === "/demo-wallet/send-transaction") {
      await handleDemoWalletSendTransaction(req, res);
      return;
    }
    if (req.method === "POST" && req.url === "/airdrop") {
      await handleAirdrop(req, res);
      return;
    }
    json(res, 404, { error: "not_found" }, origin);
  } catch (error) {
    json(res, 400, { error: safeErrorMessage(error) }, origin);
  }
});

server.listen(port, () => {
  console.log(
    `mock-USDC faucet listening on :${port} mint=${mint.toBase58()} faucet=${faucet.publicKey.toBase58()}`,
  );
});
