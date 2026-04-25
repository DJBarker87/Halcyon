"use client";

import { Buffer } from "buffer";
import {
  BaseWalletAdapter,
  WalletConnectionError,
  WalletDisconnectedError,
  WalletName,
  WalletNotConnectedError,
  WalletReadyState,
  WalletSendTransactionError,
  type SendTransactionOptions,
} from "@solana/wallet-adapter-base";
import {
  Connection,
  PublicKey,
  Transaction,
  VersionedTransaction,
  type TransactionSignature,
  type TransactionVersion,
} from "@solana/web3.js";

export const DemoWalletName = "Demo Wallet" as WalletName<"Demo Wallet">;

const DEMO_WALLET_ICON =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96">
      <rect width="96" height="96" rx="18" fill="#0f766e"/>
      <path fill="#f8fafc" d="M22 28h52a8 8 0 0 1 8 8v24a8 8 0 0 1-8 8H22a8 8 0 0 1-8-8V36a8 8 0 0 1 8-8Zm5 12v16h24V40H27Zm33 5h12v6H60v-6Z"/>
    </svg>`,
  );

type DemoWalletStatus = {
  ok: boolean;
  enabled: boolean;
  wallet?: string;
  reason?: string;
};

type DemoWalletSendResponse = {
  ok: boolean;
  signature?: string;
  error?: string;
};

export class DemoWalletAdapter extends BaseWalletAdapter<"Demo Wallet"> {
  name = DemoWalletName;
  url = "https://halcyonprotocol.xyz/faucet";
  icon = DEMO_WALLET_ICON;
  supportedTransactionVersions = new Set(["legacy", 0] as TransactionVersion[]);

  private endpoint: string;
  private key: PublicKey | null = null;
  private connectingState = false;

  constructor(endpoint = "/faucet-api/demo-wallet") {
    super();
    this.endpoint = endpoint.replace(/\/+$/, "");
  }

  get publicKey() {
    return this.key;
  }

  get connecting() {
    return this.connectingState;
  }

  get readyState() {
    return typeof window === "undefined" ? WalletReadyState.Unsupported : WalletReadyState.Loadable;
  }

  async connect() {
    if (this.connected || this.connectingState) return;
    this.connectingState = true;
    try {
      const response = await fetch(`${this.endpoint}/status`, { cache: "no-store" });
      const status = (await response.json()) as DemoWalletStatus;
      if (!response.ok || !status.ok || !status.enabled || !status.wallet) {
        throw new Error(status.reason ?? "Demo Wallet is not configured");
      }
      this.key = new PublicKey(status.wallet);
      this.emit("connect", this.key);
    } catch (error) {
      this.emit("error", new WalletConnectionError("Demo Wallet connection failed", error));
      throw error;
    } finally {
      this.connectingState = false;
    }
  }

  async disconnect() {
    if (!this.key) return;
    this.key = null;
    this.emit("disconnect");
  }

  async sendTransaction(
    transaction: Transaction | VersionedTransaction,
    _connection: Connection,
    options: SendTransactionOptions = {},
  ): Promise<TransactionSignature> {
    if (!this.key) {
      throw new WalletNotConnectedError();
    }

    try {
      const isVersioned = "version" in transaction;
      if (isVersioned) {
        if (options.signers?.length) {
          transaction.sign(options.signers);
        }
      } else {
        if (options.signers?.length) {
          transaction.partialSign(...options.signers);
        }
      }

      const serialized = isVersioned
        ? Buffer.from(transaction.serialize()).toString("base64")
        : Buffer.from(
            transaction.serialize({
              requireAllSignatures: false,
              verifySignatures: false,
            }),
          ).toString("base64");

      const response = await fetch(`${this.endpoint}/send-transaction`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ transaction: serialized }),
      });
      const payload = (await response.json()) as DemoWalletSendResponse;
      if (!response.ok || !payload.ok || !payload.signature) {
        throw new Error(payload.error ?? "Demo Wallet transaction failed");
      }
      return payload.signature;
    } catch (error) {
      this.emit("error", new WalletSendTransactionError("Demo Wallet transaction failed", error));
      throw error;
    }
  }

  async autoConnect() {
    try {
      await this.connect();
    } catch (error) {
      this.emit("error", new WalletDisconnectedError("Demo Wallet auto-connect failed", error));
    }
  }
}
