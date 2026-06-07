import { invoke } from '@tauri-apps/api/core';

export const CONTROL_PROTOCOL_VERSION = 1;

export type ControlOperation =
  | 'get_dashboard_status'
  | 'get_settings'
  | 'update_settings'
  | 'enroll_windows_credential';

export type ControlOperationStatus =
  | 'completed'
  | 'failed'
  | 'requires_elevation'
  | 'requires_user_input'
  | 'service_unavailable'
  | 'permission_denied'
  | 'invalid_request'
  | 'unsupported_protocol'
  | 'cancelled';

export interface ControlRequestEnvelope<TPayload = unknown> {
  protocol_version: number;
  correlation_id: string;
  operation: ControlOperation;
  payload: TPayload;
}

export interface ControlResponseEnvelope<TDetails = unknown> {
  protocol_version: number;
  correlation_id: string;
  operation: ControlOperation;
  operation_status: ControlOperationStatus;
  message: string;
  safe_details: TDetails;
  next_recommended_action?: string;
}

export type LogonWakeMode = 'input_triggered';

export interface ControlSettingsSnapshot {
  presence_lock_enabled: boolean;
  logon_wake_mode?: LogonWakeMode;
}

export interface ControlSettingsPatch {
  presence_lock_enabled?: boolean;
  logon_wake_mode?: LogonWakeMode;
}

export type WindowsCredentialAccountType = 'local' | 'microsoft_account' | 'domain';

export interface WindowsCredentialEnrollmentPayload {
  windows_account_username?: string;
  user_id?: string;
  user_sid?: string;
  account_type?: WindowsCredentialAccountType;
  credential_ref?: string;
}

export interface WindowsCredentialEnrollmentOutcome {
  windows_account_username: string;
  user_id: string;
  user_sid: string;
  account_type: WindowsCredentialAccountType;
  credential_ref: string;
}

export function isControlRuntimeAvailable() {
  return isTauriRuntime();
}

export async function getControlSettings() {
  return sendControlRequest<ControlSettingsSnapshot>('get_settings', {});
}

export async function updateControlSettings(patch: ControlSettingsPatch) {
  return sendControlRequest<ControlSettingsSnapshot, ControlSettingsPatch>('update_settings', patch);
}

export async function enrollWindowsCredential(passwordSecret: string) {
  if (!isTauriRuntime()) {
    throw new Error('Tauri 运行时未连接。');
  }

  const request: ControlRequestEnvelope<WindowsCredentialEnrollmentPayload> = {
    protocol_version: CONTROL_PROTOCOL_VERSION,
    correlation_id: `control-ui-event-enroll_windows_credential-${Date.now()}`,
    operation: 'enroll_windows_credential',
    payload: {},
  };

  return invoke<ControlResponseEnvelope<WindowsCredentialEnrollmentOutcome>>(
    'handle_credential_enrollment_request',
    { request, passwordSecret },
  );
}

export async function sendControlRequest<TDetails = unknown, TPayload = unknown>(
  operation: ControlOperation,
  payload: TPayload,
): Promise<ControlResponseEnvelope<TDetails>> {
  if (!isTauriRuntime()) {
    throw new Error('Tauri 运行时未连接。');
  }

  const request: ControlRequestEnvelope<TPayload> = {
    protocol_version: CONTROL_PROTOCOL_VERSION,
    correlation_id: `control-ui-event-${operation}-${Date.now()}`,
    operation,
    payload,
  };

  return invoke<ControlResponseEnvelope<TDetails>>('handle_control_request', { request });
}

function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
