export const CONTROL_PROTOCOL_VERSION = 1;

export type ControlOperation =
  | 'get_dashboard_status'
  | 'get_settings'
  | 'update_settings'
  | 'get_windows_credential_account'
  | 'enroll_windows_credential'
  | 'list_face_templates'
  | 'delete_face_template'
  | 'list_cameras'
  | 'start_face_enrollment'
  | 'get_face_enrollment_status'
  | 'get_face_enrollment_preview'
  | 'cancel_face_enrollment'
  | 'finish_face_enrollment'
  | 'run_face_auth_self_test';

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

export type LogonWakeMode = 'input_triggered' | 'background_policy' | 'hybrid';

export interface ControlSettingsSnapshot {
  presence_lock_enabled: boolean;
  logon_wake_mode?: LogonWakeMode;
}

export interface ControlSettingsPatch {
  presence_lock_enabled?: boolean;
  logon_wake_mode?: LogonWakeMode;
}

export type WindowsCredentialAccountType = 'local' | 'microsoft_account' | 'domain';
export type WindowsCredentialSecretState = 'configured' | 'not_configured';

export interface WindowsCredentialEnrollmentPayload {
  windows_account_username?: string;
  user_id?: string;
  user_sid?: string;
  account_type?: WindowsCredentialAccountType;
  credential_ref?: string;
}

export interface WindowsCredentialAccountProfile {
  windows_account_username: string;
  user_id: string;
  user_sid: string;
  account_type: WindowsCredentialAccountType;
  credential_ref: string;
  credential_secret_state: WindowsCredentialSecretState;
}

export interface WindowsCredentialEnrollmentOutcome {
  windows_account_username: string;
  user_id: string;
  user_sid: string;
  account_type: WindowsCredentialAccountType;
  credential_ref: string;
  credential_secret_state: WindowsCredentialSecretState;
}

export type FaceTemplateKind = 'selected_template_set' | 'repository_template';
export type FaceTemplateSourceState = 'active_service_template' | 'repository_template';

export interface FaceRecognitionModelSummary {
  model_family: string;
  model_version: string;
}

export interface FaceTemplateSummary {
  face_template_ref: string;
  user_id: string;
  display_name?: string;
  template_kind: FaceTemplateKind;
  recognition_model: FaceRecognitionModelSummary;
  selected_template_count: number;
  rejected_sample_count?: number;
  created_at_unix_ms?: number;
  updated_at_unix_ms?: number;
  source_state: FaceTemplateSourceState;
}

export interface FaceTemplateList {
  templates: FaceTemplateSummary[];
}

export interface DeleteFaceTemplatePayload {
  face_template_ref: string;
}

export interface DeleteFaceTemplateOutcome {
  face_template_ref: string;
  template_deleted: boolean;
  service_auth_requires_reconfiguration: boolean;
}

export interface CameraDeviceSummary {
  camera_id: string;
  display_name: string;
}

export interface CameraDeviceList {
  cameras: CameraDeviceSummary[];
}

export type FaceEnrollmentProfile = 'guided_standard';

export interface FaceEnrollmentStartPayload {
  user_id?: string;
  camera_id?: string;
  enrollment_profile?: FaceEnrollmentProfile;
  allow_partial_enrollment?: boolean;
}

export interface FaceEnrollmentSessionPayload {
  enrollment_session_id: string;
}

export type FaceEnrollmentSessionState =
  | 'starting'
  | 'running'
  | 'waiting_for_face'
  | 'waiting_for_pose'
  | 'capturing'
  | 'finishing'
  | 'completed'
  | 'failed'
  | 'cancelled';

export type FaceEnrollmentFrameResult =
  | 'face_accepted'
  | 'no_face_detected'
  | 'multiple_faces_detected'
  | 'pose_not_ready'
  | 'quality_rejected'
  | 'model_unavailable';

export interface FaceTemplateEnrollmentSummary {
  selected_template_count: number;
  rejected_sample_count: number;
}

export interface FaceEnrollmentSessionStatus {
  enrollment_session_id: string;
  session_state: FaceEnrollmentSessionState;
  user_id: string;
  camera_id: string;
  current_step?: string;
  current_instruction_code?: string;
  accepted_sample_count: number;
  required_sample_count?: number;
  last_frame_result?: FaceEnrollmentFrameResult;
  template_summary?: FaceTemplateEnrollmentSummary;
}

export interface FaceEnrollmentPreviewFrame {
  enrollment_session_id: string;
  preview_available: boolean;
  mime_type?: string;
  image_base64?: string;
  frame_updated_at_unix_ms?: number;
}

export interface FaceEnrollmentFinishOutcome {
  enrollment_session_id: string;
  session_state: FaceEnrollmentSessionState;
  face_template_ref: string;
  user_id: string;
  template_summary: FaceTemplateEnrollmentSummary;
  service_auth_configured: boolean;
  service_auth_configuration_error?: string;
}

export interface FaceAuthSelfTestPayload {
  session_id?: string;
  require_credential_ready?: boolean;
  camera_id?: string;
}

export interface FaceAuthSelfTestOutcome {
  session_id: string;
  auth_match_passed: boolean;
  grant_issued: boolean;
  credential_material_ready: boolean;
  credential_decryption_succeeded: boolean;
  pipe_delivery_confirmed: boolean;
  best_match_score?: number;
  matched_face_template_ref?: string;
}
