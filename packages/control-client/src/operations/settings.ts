import type { ControlSettingsPatch, ControlSettingsSnapshot } from '../protocol/types';
import type { ControlTransport } from '../transport/ControlTransport';
import { sendControlRequest } from './request';

export function getControlSettings(transport: ControlTransport) {
  return sendControlRequest<ControlSettingsSnapshot>(transport, 'get_settings', {});
}

export function updateControlSettings(
  transport: ControlTransport,
  patch: ControlSettingsPatch,
) {
  return sendControlRequest<ControlSettingsSnapshot, ControlSettingsPatch>(
    transport,
    'update_settings',
    patch,
  );
}
