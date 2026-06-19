import type {
  FaceEnrollmentFinishOutcome,
  FaceEnrollmentPreviewFrame,
  FaceEnrollmentSessionPayload,
  FaceEnrollmentSessionStatus,
  FaceEnrollmentStartPayload,
} from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';
import { sendControlRequest } from './request';

export function startFaceEnrollment(
  transport: ControlTransport,
  payload: FaceEnrollmentStartPayload = {},
) {
  return sendControlRequest<FaceEnrollmentSessionStatus, FaceEnrollmentStartPayload>(
    transport,
    'start_face_enrollment',
    payload,
  );
}

export function getFaceEnrollmentStatus(
  transport: ControlTransport,
  payload: FaceEnrollmentSessionPayload,
) {
  return sendControlRequest<FaceEnrollmentSessionStatus, FaceEnrollmentSessionPayload>(
    transport,
    'get_face_enrollment_status',
    payload,
  );
}

export function getFaceEnrollmentPreview(
  transport: ControlTransport,
  payload: FaceEnrollmentSessionPayload,
) {
  return sendControlRequest<FaceEnrollmentPreviewFrame, FaceEnrollmentSessionPayload>(
    transport,
    'get_face_enrollment_preview',
    payload,
  );
}

export function cancelFaceEnrollment(
  transport: ControlTransport,
  payload: FaceEnrollmentSessionPayload,
) {
  return sendControlRequest<FaceEnrollmentSessionStatus, FaceEnrollmentSessionPayload>(
    transport,
    'cancel_face_enrollment',
    payload,
  );
}

export function finishFaceEnrollment(
  transport: ControlTransport,
  payload: FaceEnrollmentSessionPayload,
) {
  return sendControlRequest<FaceEnrollmentFinishOutcome, FaceEnrollmentSessionPayload>(
    transport,
    'finish_face_enrollment',
    payload,
  );
}
