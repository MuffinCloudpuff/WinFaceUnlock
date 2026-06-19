import type {
  ControlRequestEnvelope,
  ControlResponseEnvelope,
  WindowsCredentialEnrollmentPayload,
  WindowsCredentialEnrollmentOutcome,
} from '../protocol/types';

export interface ControlTransport {
  send<TDetails = unknown, TPayload = unknown>(
    request: ControlRequestEnvelope<TPayload>,
  ): Promise<ControlResponseEnvelope<TDetails>>;

  sendCredentialEnrollment(
    request: ControlRequestEnvelope<WindowsCredentialEnrollmentPayload>,
    passwordSecret: string,
  ): Promise<ControlResponseEnvelope<WindowsCredentialEnrollmentOutcome>>;

  isAvailable(): boolean;
}
