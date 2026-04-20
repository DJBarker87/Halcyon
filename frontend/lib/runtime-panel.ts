export const HALCYON_OPEN_RUNTIME_PANEL = "halcyon-open-runtime-panel";

export function openRuntimeConfigPanel() {
  window.dispatchEvent(new CustomEvent(HALCYON_OPEN_RUNTIME_PANEL));
}
