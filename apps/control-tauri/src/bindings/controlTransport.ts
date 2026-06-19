import { createTauriControlTransport } from '@winfaceunlock/control-tauri-transport';

export const controlTransport = createTauriControlTransport();

export function isControlRuntimeAvailable() {
  return controlTransport.isAvailable();
}
