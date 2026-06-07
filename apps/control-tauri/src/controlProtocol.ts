import { invoke } from '@tauri-apps/api/core';

export const CONTROL_PROTOCOL_VERSION = 1;

export type ControlOperation = 'get_dashboard_status';

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

export interface ControlRequestEnvelope {
  protocol_version: number;
  correlation_id: string;
  operation: ControlOperation;
  payload: unknown;
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

export interface DashboardStatus {
  service: {
    installation_state: 'installed' | 'missing';
    runtime_state:
      | 'running'
      | 'stopped'
      | 'paused'
      | 'start_pending'
      | 'stop_pending'
      | 'missing'
      | string;
    process_id?: number;
  };
  provider: {
    registration_state: 'registered' | 'partially_registered' | 'not_registered';
    credential_provider_registered: boolean;
    com_server_registered: boolean;
    project_config_registered: boolean;
  };
  service_config: {
    registry_config_state: 'present' | 'missing';
    auth_mode?: string;
    face_template_path?: string;
    presence_lock_enabled?: boolean;
    presence_detector_kind?: string;
    presence_tracking_mode?: string;
  };
  data_directory: {
    program_data_dir?: string;
    program_data_presence: 'present' | 'missing' | 'unknown';
    presence_audit_dir?: string;
    presence_audit_presence: 'present' | 'missing' | 'unknown';
  };
  presence_runtime?: {
    monitor_state: 'running' | 'stopped' | 'disabled' | 'unavailable' | string;
    session_id?: number;
    reason?: string;
    updated_at_unix_ms?: number;
  };
}

export type DashboardResponseEnvelope = ControlResponseEnvelope<DashboardStatus>;

export async function loadDashboardStatus(): Promise<DashboardResponseEnvelope> {
  if (!isTauriRuntime()) {
    throw new Error('Tauri 运行时未连接。');
  }

  const request: ControlRequestEnvelope = {
    protocol_version: CONTROL_PROTOCOL_VERSION,
    correlation_id: `control-ui-${Date.now()}`,
    operation: 'get_dashboard_status',
    payload: {},
  };

  return invoke<DashboardResponseEnvelope>('handle_control_request', { request });
}

function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
