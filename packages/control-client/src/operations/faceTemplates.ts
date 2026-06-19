import type {
  DeleteFaceTemplateOutcome,
  DeleteFaceTemplatePayload,
  FaceTemplateList,
} from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';
import { sendControlRequest } from './request';

export function listFaceTemplates(transport: ControlTransport) {
  return sendControlRequest<FaceTemplateList>(transport, 'list_face_templates', {});
}

export function deleteFaceTemplate(
  transport: ControlTransport,
  payload: DeleteFaceTemplatePayload,
) {
  return sendControlRequest<DeleteFaceTemplateOutcome, DeleteFaceTemplatePayload>(
    transport,
    'delete_face_template',
    payload,
  );
}
