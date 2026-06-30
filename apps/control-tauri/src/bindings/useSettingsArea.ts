import {
  deleteFaceTemplate,
  getControlSettings,
  listFaceTemplates,
  type FaceTemplateSummary,
  type LogonWakeMode,
  updateControlSettings,
  listIntruderSnapshots,
  deleteIntruderSnapshot,
} from '@winfaceunlock/control-client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { controlTransport, isControlRuntimeAvailable } from './controlTransport';
import { subscribeFaceTemplatesChanged } from './faceTemplateEvents';

export type TriggerMode = 'keyboard' | 'silent';

export interface EnrolledFaceViewModel {
  id: string;
  name: string;
  avatarImageSrc?: string;
}

export interface IntruderSnapshotViewModel {
  id: string;
  time: string;
  timestampMs: number;
  avatarSrc: string;
}

export interface SettingsAreaViewModel {
  autoLock: boolean;
  intruderSnap: boolean;
  triggerMode: TriggerMode;
  logonFaceMatchThreshold: number;
  enrolledFaces: EnrolledFaceViewModel[];
  intruders: IntruderSnapshotViewModel[];
  setIntruderSnap: (enabled: boolean) => void;
  changeAutoLock: (enabled: boolean) => void;
  changeTriggerMode: (mode: TriggerMode) => void;
  changeLogonFaceMatchThreshold: (threshold: number) => void;
  deleteFace: (faceTemplateRef: string) => void;
  deleteIntruder: (id: string) => void;
}

export function useSettingsArea(): SettingsAreaViewModel {
  const [autoLock, setAutoLock] = useState(true);
  const autoLockRequestId = useRef(0);
  const [intruderSnap, setIntruderSnap] = useState(true);
  const intruderSnapRequestId = useRef(0);
  const [triggerMode, setTriggerMode] = useState<TriggerMode>('keyboard');
  const triggerModeRequestId = useRef(0);
  const [logonFaceMatchThreshold, setLogonFaceMatchThreshold] = useState(0.75);
  const logonFaceMatchThresholdRequestId = useRef(0);
  const faceDeleteRequestId = useRef(0);
  const [enrolledFaces, setEnrolledFaces] = useState<EnrolledFaceViewModel[]>([]);
  const [intruders, setIntruders] = useState<IntruderSnapshotViewModel[]>([]);

  const loadFaceTemplates = useCallback(() => {
    if (!isControlRuntimeAvailable()) {
      return Promise.resolve();
    }

    return listFaceTemplates(controlTransport)
      .then((response) => {
        if (response.operation_status !== 'completed') {
          console.warn('WinFaceUnlock face templates were not loaded.', response);
          return;
        }
        setEnrolledFaces(response.safe_details.templates.map(faceTemplateToViewModel));
      })
      .catch((error) => {
        console.warn('Failed to load WinFaceUnlock face templates.', error);
      });
  }, []);

  const loadIntruders = useCallback(() => {
    if (!isControlRuntimeAvailable()) {
      return Promise.resolve();
    }

    return listIntruderSnapshots(controlTransport)
      .then((res) => {
        const now = new Date();
        const mapped = res.safe_details.snapshots.map((s) => {
          const date = new Date(s.timestamp_ms);
          const isToday =
            date.getDate() === now.getDate() &&
            date.getMonth() === now.getMonth() &&
            date.getFullYear() === now.getFullYear();
          const isYesterday =
            new Date(now.getTime() - 86400000).getDate() === date.getDate() &&
            new Date(now.getTime() - 86400000).getMonth() === date.getMonth() &&
            new Date(now.getTime() - 86400000).getFullYear() === date.getFullYear();

          const timeStr = `${date.getHours().toString().padStart(2, '0')}:${date
            .getMinutes()
            .toString()
            .padStart(2, '0')}`;
          
          let displayTime = '';
          if (isToday) displayTime = `今天 ${timeStr}`;
          else if (isYesterday) displayTime = `昨天 ${timeStr}`;
          else displayTime = `${date.getMonth() + 1}-${date.getDate()} ${timeStr}`;

          return {
            id: s.id,
            time: displayTime,
            timestampMs: s.timestamp_ms,
            avatarSrc: s.avatar_preview_base64,
          };
        });
        setIntruders(mapped);
      })
      .catch((err) => {
        console.error('Failed to load intruders:', err);
      });
  }, []);

  useEffect(() => {
    if (!isControlRuntimeAvailable()) {
      return;
    }

    let isMounted = true;
    getControlSettings(controlTransport)
      .then((response) => {
        if (!isMounted || response.operation_status !== 'completed') {
          return;
        }
        setAutoLock(response.safe_details.presence_lock_enabled);
        if (response.safe_details.intruder_snap_enabled !== undefined) {
          setIntruderSnap(response.safe_details.intruder_snap_enabled);
        }
        const backendTriggerMode = logonWakeModeToTriggerMode(
          response.safe_details.logon_wake_mode,
        );
        if (backendTriggerMode) {
          setTriggerMode(backendTriggerMode);
        }
        setLogonFaceMatchThreshold(
          normalizeLogonFaceMatchThreshold(response.safe_details.logon_face_match_threshold),
        );
      })
      .catch((error) => {
        console.warn('Failed to load WinFaceUnlock settings.', error);
      });

    loadFaceTemplates();
    loadIntruders();
  }, [loadFaceTemplates, loadIntruders]);



  useEffect(() => subscribeFaceTemplatesChanged(loadFaceTemplates), [loadFaceTemplates]);

  const changeAutoLock = useCallback(
    (nextChecked: boolean) => {
      const previousChecked = autoLock;
      setAutoLock(nextChecked);

      if (!isControlRuntimeAvailable()) {
        return;
      }

      const requestId = autoLockRequestId.current + 1;
      autoLockRequestId.current = requestId;

      updateControlSettings(controlTransport, { presence_lock_enabled: nextChecked })
        .then((response) => {
          if (requestId !== autoLockRequestId.current) {
            return;
          }

          if (response.operation_status === 'completed') {
            setAutoLock(response.safe_details.presence_lock_enabled);
            return;
          }

          setAutoLock(previousChecked);
          console.warn('WinFaceUnlock settings update was not completed.', response);
        })
        .catch((error) => {
          if (requestId !== autoLockRequestId.current) {
            return;
          }
          setAutoLock(previousChecked);
          console.warn('Failed to update WinFaceUnlock settings.', error);
        });
    },
    [autoLock],
  );

  const changeTriggerMode = useCallback(
    (nextMode: TriggerMode) => {
      const previousMode = triggerMode;
      setTriggerMode(nextMode);

      if (!isControlRuntimeAvailable()) {
        return;
      }

      const requestId = triggerModeRequestId.current + 1;
      triggerModeRequestId.current = requestId;

      updateControlSettings(controlTransport, { logon_wake_mode: triggerModeToLogonWakeMode(nextMode) })
        .then((response) => {
          if (requestId !== triggerModeRequestId.current) {
            return;
          }

          if (response.operation_status === 'completed') {
            setTriggerMode(
              logonWakeModeToTriggerMode(response.safe_details.logon_wake_mode) ?? nextMode,
            );
            return;
          }

          setTriggerMode(previousMode);
          console.warn('WinFaceUnlock logon wake mode update was not completed.', response);
        })
        .catch((error) => {
          if (requestId !== triggerModeRequestId.current) {
            return;
          }
          setTriggerMode(previousMode);
          console.warn('Failed to update WinFaceUnlock logon wake mode.', error);
        });
    },
    [triggerMode],
  );

  const changeLogonFaceMatchThreshold = useCallback(
    (nextThreshold: number) => {
      const normalizedThreshold = normalizeLogonFaceMatchThreshold(nextThreshold);
      const previousThreshold = logonFaceMatchThreshold;
      setLogonFaceMatchThreshold(normalizedThreshold);

      if (!isControlRuntimeAvailable()) {
        return;
      }

      const requestId = logonFaceMatchThresholdRequestId.current + 1;
      logonFaceMatchThresholdRequestId.current = requestId;

      updateControlSettings(controlTransport, {
        logon_face_match_threshold: normalizedThreshold,
      })
        .then((response) => {
          if (requestId !== logonFaceMatchThresholdRequestId.current) {
            return;
          }

          if (response.operation_status === 'completed') {
            setLogonFaceMatchThreshold(
              normalizeLogonFaceMatchThreshold(
                response.safe_details.logon_face_match_threshold,
              ),
            );
            return;
          }

          setLogonFaceMatchThreshold(previousThreshold);
          console.warn(
            'WinFaceUnlock logon face match threshold update was not completed.',
            response,
          );
        })
        .catch((error) => {
          if (requestId !== logonFaceMatchThresholdRequestId.current) {
            return;
          }
          setLogonFaceMatchThreshold(previousThreshold);
          console.warn(
            'Failed to update WinFaceUnlock logon face match threshold.',
            error,
          );
        });
    },
    [logonFaceMatchThreshold],
  );

  const deleteFace = useCallback((faceTemplateRef: string) => {
    if (!isControlRuntimeAvailable()) {
      setEnrolledFaces((currentFaces) =>
        currentFaces.filter((face) => face.id !== faceTemplateRef),
      );
      return;
    }

    const requestId = faceDeleteRequestId.current + 1;
    faceDeleteRequestId.current = requestId;

    deleteFaceTemplate(controlTransport, { face_template_ref: faceTemplateRef })
      .then((response) => {
        if (requestId !== faceDeleteRequestId.current) {
          return;
        }

        if (
          response.operation_status === 'completed' &&
          response.safe_details.template_deleted
        ) {
          setEnrolledFaces((currentFaces) =>
            currentFaces.filter((face) => face.id !== faceTemplateRef),
          );
          return;
        }

        console.warn('WinFaceUnlock face template delete was not completed.', response);
      })
      .catch((error) => {
        if (requestId !== faceDeleteRequestId.current) {
          return;
        }
        console.warn('Failed to delete WinFaceUnlock face template.', error);
      });
  }, []);

  const changeIntruderSnap = useCallback(
    (nextChecked: boolean) => {
      const previousChecked = intruderSnap;
      setIntruderSnap(nextChecked);

      if (!isControlRuntimeAvailable()) {
        return;
      }

      const requestId = intruderSnapRequestId.current + 1;
      intruderSnapRequestId.current = requestId;

      updateControlSettings(controlTransport, { intruder_snap_enabled: nextChecked })
        .then((response) => {
          if (requestId !== intruderSnapRequestId.current) {
            return;
          }

          if (response.operation_status === 'completed') {
            if (response.safe_details.intruder_snap_enabled !== undefined) {
              setIntruderSnap(response.safe_details.intruder_snap_enabled);
            }
            return;
          }

          setIntruderSnap(previousChecked);
          console.warn('WinFaceUnlock intruder snap settings update was not completed.', response);
        })
        .catch((error) => {
          if (requestId !== intruderSnapRequestId.current) {
            return;
          }
          setIntruderSnap(previousChecked);
          console.warn('Failed to update WinFaceUnlock intruder snap settings.', error);
        });
    },
    [intruderSnap],
  );

  const deleteIntruder = useCallback((id: string) => {
    if (!isControlRuntimeAvailable()) {
      setIntruders((current) => current.filter((i) => i.id !== id));
      return;
    }
    deleteIntruderSnapshot(controlTransport, { id })
      .then((res) => {
        if (res.operation_status === 'completed') {
          setIntruders((current) => current.filter((i) => i.id !== id));
        }
      })
      .catch((err) => console.error(err));
  }, []);

  return {
    autoLock,
    intruderSnap,
    triggerMode,
    logonFaceMatchThreshold,
    enrolledFaces,
    intruders,
    setIntruderSnap: changeIntruderSnap,
    changeAutoLock,
    changeTriggerMode,
    changeLogonFaceMatchThreshold,
    deleteFace,
    deleteIntruder,
  };
}

function normalizeLogonFaceMatchThreshold(threshold: number): number {
  if (!Number.isFinite(threshold)) {
    return 0.75;
  }
  return Math.min(0.9, Math.max(0.3, Number(threshold.toFixed(2))));
}

function faceTemplateToViewModel(template: FaceTemplateSummary): EnrolledFaceViewModel {
  return {
    id: template.face_template_ref,
    name:
      template.display_name ??
      `人脸 ${template.selected_template_count}`,
    avatarImageSrc: faceAvatarPreviewToImageSrc(template),
  };
}

function faceAvatarPreviewToImageSrc(template: FaceTemplateSummary): string | undefined {
  const preview = template.avatar_preview;
  if (!preview?.mime_type || !preview.image_base64) {
    return undefined;
  }

  return `data:${preview.mime_type};base64,${preview.image_base64}`;
}

function logonWakeModeToTriggerMode(mode?: LogonWakeMode): TriggerMode | undefined {
  if (mode === 'triggered_recognition' || mode === 'input_triggered') {
    return 'keyboard';
  }
  if (
    mode === 'background_silent_recognition' ||
    mode === 'background_policy' ||
    mode === 'hybrid'
  ) {
    return 'silent';
  }
  return undefined;
}

function triggerModeToLogonWakeMode(mode: TriggerMode): LogonWakeMode {
  if (mode === 'silent') {
    return 'background_silent_recognition';
  }
  return 'triggered_recognition';
}
