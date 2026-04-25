import fs from "node:fs";
import path from "node:path";
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";

const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const MINT_SIZE = 82;
const DECIMALS = 6;

function env(name, fallback = "") {
  const value = process.env[name]?.trim();
  return value ? value : fallback;
}

function loadKeypair(path) {
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(path, "utf8"))));
}

function initializeMintInstruction(mint, mintAuthority) {
  const data = Buffer.alloc(67);
  data.writeUInt8(20, 0);
  data.writeUInt8(DECIMALS, 1);
  Buffer.from(mintAuthority.toBytes()).copy(data, 2);
  data.writeUInt8(0, 34);
  return new TransactionInstruction({
    programId: TOKEN_PROGRAM_ID,
    keys: [{ pubkey: mint, isSigner: false, isWritable: true }],
    data,
  });
}

function loadOrCreateFaucetKeypair(filePath) {
  if (fs.existsSync(filePath)) return loadKeypair(filePath);
  const kp = Keypair.generate();
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, JSON.stringify(Array.from(kp.secretKey)));
  fs.chmodSync(filePath, 0o600);
  return kp;
}

const rpcUrl = env("RPC_URL", env("HELIUS_DEVNET_RPC", "https://api.devnet.solana.com"));
const payerPath = env("PAYER_KEYPAIR_PATH", `${process.env.HOME}/.config/solana/id.json`);
const faucetPath = env("MOCK_USDC_FAUCET_KEYPAIR_PATH", "ops/devnet_keys/mock-usdc-faucet.json");
const mintPath = env("MOCK_USDC_MINT_KEYPAIR_PATH", "ops/devnet_keys/mock-usdc-mint.json");

const connection = new Connection(rpcUrl, "confirmed");
const payer = loadKeypair(payerPath);
const faucet = loadOrCreateFaucetKeypair(faucetPath);
const mint = fs.existsSync(mintPath) ? loadKeypair(mintPath) : Keypair.generate();
if (!fs.existsSync(mintPath)) {
  fs.mkdirSync(path.dirname(mintPath), { recursive: true });
  fs.writeFileSync(mintPath, JSON.stringify(Array.from(mint.secretKey)));
  fs.chmodSync(mintPath, 0o600);
}

const existing = await connection.getAccountInfo(mint.publicKey, "confirmed");
let signature = null;
if (!existing) {
  const rent = await connection.getMinimumBalanceForRentExemption(MINT_SIZE);
  const tx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: mint.publicKey,
      lamports: rent,
      space: MINT_SIZE,
      programId: TOKEN_PROGRAM_ID,
    }),
    initializeMintInstruction(mint.publicKey, faucet.publicKey),
  );
  signature = await sendAndConfirmTransaction(connection, tx, [payer, mint], {
    commitment: "confirmed",
  });
}

console.log(JSON.stringify(
  {
    ok: true,
    created: !existing,
    signature,
    mint: mint.publicKey.toBase58(),
    faucetAuthority: faucet.publicKey.toBase58(),
    faucetKeypairPath: faucetPath,
    mintKeypairPath: mintPath,
    next: [
      `export USDC_MINT=${mint.publicKey.toBase58()}`,
      `export MOCK_USDC_MINT=${mint.publicKey.toBase58()}`,
      `export MOCK_USDC_FAUCET_KEYPAIR_PATH=${faucetPath}`,
      "halcyon init-payment-mint --rpc <devnet-rpc> --keypair <admin-keypair>",
    ],
  },
  null,
  2,
));
