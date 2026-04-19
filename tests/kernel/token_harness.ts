import {
  Commitment,
  ConfirmOptions,
  Connection,
  Keypair,
  PublicKey,
  Signer,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";

export const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
);
const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
);

const MINT_SIZE = 82;

export type TokenAccount = {
  address: PublicKey;
  mint: PublicKey;
  owner: PublicKey;
  amount: bigint;
};

function pushPubkey(target: Buffer, offset: number, key: PublicKey): number {
  key.toBuffer().copy(target, offset);
  return offset + 32;
}

function createInitializeMint2Instruction(
  mint: PublicKey,
  decimals: number,
  mintAuthority: PublicKey,
  freezeAuthority: PublicKey | null
): TransactionInstruction {
  const data = Buffer.alloc(freezeAuthority ? 67 : 35);
  data[0] = 20; // TokenInstruction::InitializeMint2
  data[1] = decimals;
  let offset = pushPubkey(data, 2, mintAuthority);
  data[offset] = freezeAuthority ? 1 : 0;
  offset += 1;
  if (freezeAuthority) {
    pushPubkey(data, offset, freezeAuthority);
  }
  return new TransactionInstruction({
    programId: TOKEN_PROGRAM_ID,
    keys: [{ pubkey: mint, isSigner: false, isWritable: true }],
    data,
  });
}

function createAssociatedTokenAccountInstruction(
  payer: PublicKey,
  associatedToken: PublicKey,
  owner: PublicKey,
  mint: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: associatedToken, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.alloc(0),
  });
}

function createMintToInstruction(
  mint: PublicKey,
  destination: PublicKey,
  authority: PublicKey,
  amount: bigint
): TransactionInstruction {
  const data = Buffer.alloc(9);
  data[0] = 7; // TokenInstruction::MintTo
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

function decodeTokenAccount(address: PublicKey, data: Buffer): TokenAccount {
  if (data.length < 72) {
    throw new Error(`token account ${address} has invalid size ${data.length}`);
  }
  return {
    address,
    mint: new PublicKey(data.subarray(0, 32)),
    owner: new PublicKey(data.subarray(32, 64)),
    amount: data.readBigUInt64LE(64),
  };
}

export function getAssociatedTokenAddressSync(
  mint: PublicKey,
  owner: PublicKey
): PublicKey {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID
  )[0];
}

export async function createMint(
  connection: Connection,
  payer: Signer,
  mintAuthority: PublicKey,
  freezeAuthority: PublicKey | null,
  decimals: number,
  keypair = Keypair.generate(),
  confirmOptions?: ConfirmOptions
): Promise<PublicKey> {
  const lamports = await connection.getMinimumBalanceForRentExemption(
    MINT_SIZE
  );
  const transaction = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: keypair.publicKey,
      space: MINT_SIZE,
      lamports,
      programId: TOKEN_PROGRAM_ID,
    }),
    createInitializeMint2Instruction(
      keypair.publicKey,
      decimals,
      mintAuthority,
      freezeAuthority
    )
  );
  await sendAndConfirmTransaction(
    connection,
    transaction,
    [payer, keypair],
    confirmOptions
  );
  return keypair.publicKey;
}

export async function getAccount(
  connection: Connection,
  address: PublicKey,
  commitment?: Commitment
): Promise<TokenAccount> {
  const info = await connection.getAccountInfo(address, commitment);
  if (!info) {
    throw new Error(`token account ${address} not found`);
  }
  if (!info.owner.equals(TOKEN_PROGRAM_ID)) {
    throw new Error(
      `token account ${address} has unexpected owner ${info.owner}`
    );
  }
  return decodeTokenAccount(address, info.data);
}

export async function getOrCreateAssociatedTokenAccount(
  connection: Connection,
  payer: Signer,
  mint: PublicKey,
  owner: PublicKey,
  _allowOwnerOffCurve = false,
  commitment?: Commitment,
  confirmOptions?: ConfirmOptions
): Promise<TokenAccount> {
  const address = getAssociatedTokenAddressSync(mint, owner);
  const existing = await connection.getAccountInfo(address, commitment);
  if (!existing) {
    const transaction = new Transaction().add(
      createAssociatedTokenAccountInstruction(
        payer.publicKey,
        address,
        owner,
        mint
      )
    );
    await sendAndConfirmTransaction(
      connection,
      transaction,
      [payer],
      confirmOptions
    );
  }
  return getAccount(connection, address, commitment);
}

export async function mintTo(
  connection: Connection,
  payer: Signer,
  mint: PublicKey,
  destination: PublicKey,
  authority: Signer,
  amount: number | bigint,
  confirmOptions?: ConfirmOptions
): Promise<string> {
  const transaction = new Transaction().add(
    createMintToInstruction(
      mint,
      destination,
      authority.publicKey,
      BigInt(amount)
    )
  );
  const signers = authority.publicKey.equals(payer.publicKey)
    ? [payer]
    : [payer, authority];
  return sendAndConfirmTransaction(
    connection,
    transaction,
    signers,
    confirmOptions
  );
}
