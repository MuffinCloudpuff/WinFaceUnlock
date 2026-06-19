import {
  CONTROL_PROTOCOL_VERSION,
  type ControlOperation,
  type ControlRequestEnvelope,
  type ControlResponseEnvelope,
} from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';

export function createControlRequest<TPayload>(
  operation: ControlOperation,
  payload: TPayload,
): ControlRequestEnvelope<TPayload> {
  return {
    protocol_version: CONTROL_PROTOCOL_VERSION,
    correlation_id: `control-ui-event-${operation}-${Date.now()}`,
    operation,
    payload,
  };
}

export function sendControlRequest<TDetails = unknown, TPayload = unknown>(
  transport: ControlTransport,
  operation: ControlOperation,
  payload: TPayload,
): Promise<ControlResponseEnvelope<TDetails>> {
  return transport.send<TDetails, TPayload>(createControlRequest(operation, payload));
}
