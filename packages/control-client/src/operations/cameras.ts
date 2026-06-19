import type { CameraDeviceList } from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';
import { sendControlRequest } from './request';

export function listCameras(transport: ControlTransport) {
  return sendControlRequest<CameraDeviceList>(transport, 'list_cameras', {});
}
