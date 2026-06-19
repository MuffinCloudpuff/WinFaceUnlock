import {
  CONTROL_PROTOCOL_VERSION,
  type ControlResponseEnvelope,
  type WindowsCredentialAccountProfile,
  type WindowsCredentialEnrollmentOutcome,
  type WindowsCredentialEnrollmentPayload,
} from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';
import { sendControlRequest } from './request';

export function getWindowsCredentialAccount(transport: ControlTransport) {
  return sendControlRequest<WindowsCredentialAccountProfile>(
    transport,
    'get_windows_credential_account',
    {},
  );
}

export function enrollWindowsCredential(
  transport: ControlTransport,
  passwordSecret: string,
  accountProfile?: WindowsCredentialAccountProfile,
): Promise<ControlResponseEnvelope<WindowsCredentialEnrollmentOutcome>> {
  const payload: WindowsCredentialEnrollmentPayload = accountProfile
    ? {
        windows_account_username: accountProfile.windows_account_username,
        user_id: accountProfile.user_id,
        user_sid: accountProfile.user_sid,
        account_type: accountProfile.account_type,
        credential_ref: accountProfile.credential_ref,
      }
    : {};

  return transport.sendCredentialEnrollment(
    {
      protocol_version: CONTROL_PROTOCOL_VERSION,
      correlation_id: `control-ui-event-enroll_windows_credential-${Date.now()}`,
      operation: 'enroll_windows_credential',
      payload,
    },
    passwordSecret,
  );
}
