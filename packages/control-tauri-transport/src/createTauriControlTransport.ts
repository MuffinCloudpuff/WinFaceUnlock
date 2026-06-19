import { invoke } from '@tauri-apps/api/core';
import type {
  ControlRequestEnvelope,
  ControlResponseEnvelope,
  ControlTransport,
  WindowsCredentialEnrollmentOutcome,
  WindowsCredentialEnrollmentPayload,
} from '@winfaceunlock/control-client';

export function createTauriControlTransport(): ControlTransport {
  return {
    isAvailable: isTauriRuntime,

    send<TDetails = unknown, TPayload = unknown>(
      request: ControlRequestEnvelope<TPayload>,
    ): Promise<ControlResponseEnvelope<TDetails>> {
      if (!isTauriRuntime()) {
        throw new Error('Tauri 运行时未连接。');
      }
      return invoke<ControlResponseEnvelope<TDetails>>('handle_control_request', { request });
    },

    sendCredentialEnrollment(
      request: ControlRequestEnvelope<WindowsCredentialEnrollmentPayload>,
      passwordSecret: string,
    ): Promise<ControlResponseEnvelope<WindowsCredentialEnrollmentOutcome>> {
      if (!isTauriRuntime()) {
        throw new Error('Tauri 运行时未连接。');
      }
      return invoke<ControlResponseEnvelope<WindowsCredentialEnrollmentOutcome>>(
        'handle_credential_enrollment_request',
        { request, passwordSecret },
      );
    },
  };
}

function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
