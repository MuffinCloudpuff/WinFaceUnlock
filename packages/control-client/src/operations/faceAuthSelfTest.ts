import type { FaceAuthSelfTestOutcome, FaceAuthSelfTestPayload } from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';
import { sendControlRequest } from './request';

export function runFaceAuthSelfTest(
  transport: ControlTransport,
  payload: FaceAuthSelfTestPayload = {},
) {
  return sendControlRequest<FaceAuthSelfTestOutcome, FaceAuthSelfTestPayload>(
    transport,
    'run_face_auth_self_test',
    payload,
  );
}
