import type {
  ClearIntruderSnapshotsOutcome,
  DeleteIntruderSnapshotOutcome,
  DeleteIntruderSnapshotPayload,
  ListIntruderSnapshotsResponse,
} from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';
import { sendControlRequest } from './request';

export function listIntruderSnapshots(transport: ControlTransport) {
  return sendControlRequest<ListIntruderSnapshotsResponse>(
    transport,
    'list_intruder_snapshots',
    {},
  );
}

export function deleteIntruderSnapshot(
  transport: ControlTransport,
  payload: DeleteIntruderSnapshotPayload,
) {
  return sendControlRequest<DeleteIntruderSnapshotOutcome, DeleteIntruderSnapshotPayload>(
    transport,
    'delete_intruder_snapshot',
    payload,
  );
}

export function clearIntruderSnapshots(transport: ControlTransport) {
  return sendControlRequest<ClearIntruderSnapshotsOutcome>(
    transport,
    'clear_intruder_snapshots',
    {},
  );
}
