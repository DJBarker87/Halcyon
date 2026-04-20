export type ClusterId = "localnet" | "devnet" | "mainnet";

export type ProductKind = "flagship" | "solAutocall" | "ilProtection";

export interface ClusterConfig {
  rpcUrl: string;
  kernelProgramId: string;
  flagshipProgramId: string;
  solAutocallProgramId: string;
  ilProtectionProgramId: string;
  pythSol: string;
  pythUsdc: string;
  pythSpy: string;
  pythQqq: string;
  pythIwm: string;
}
