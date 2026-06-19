import {
  deleteFaceTemplate,
  getControlSettings,
  listFaceTemplates,
  type FaceTemplateSummary,
  type LogonWakeMode,
  updateControlSettings,
} from '@winfaceunlock/control-client';
import { useCallback, useEffect, useRef, useState } from 'react';
import { controlTransport, isControlRuntimeAvailable } from './controlTransport';
import { subscribeFaceTemplatesChanged } from './faceTemplateEvents';

export type TriggerMode = 'keyboard' | 'silent' | 'hybrid';

export interface EnrolledFaceViewModel {
  id: string;
  name: string;
}

export interface IntruderSnapshotViewModel {
  id: number;
  time: string;
}

export interface SettingsAreaViewModel {
  autoLock: boolean;
  intruderSnap: boolean;
  triggerMode: TriggerMode;
  enrolledFaces: EnrolledFaceViewModel[];
  intruders: IntruderSnapshotViewModel[];
  setIntruderSnap: (enabled: boolean) => void;
  changeAutoLock: (enabled: boolean) => void;
  changeTriggerMode: (mode: TriggerMode) => void;
  deleteFace: (faceTemplateRef: string) => void;
}

export function useSettingsArea(): SettingsAreaViewModel {
  const [autoLock, setAutoLock] = useState(true);
  const autoLockRequestId = useRef(0);
  const [intruderSnap, setIntruderSnap] = useState(true);
  const [triggerMode, setTriggerMode] = useState<TriggerMode>('hybrid');
  const triggerModeRequestId = useRef(0);
  const faceDeleteRequestId = useRef(0);
  const [enrolledFaces, setEnrolledFaces] = useState<EnrolledFaceViewModel[]>([]);

  const [intruders] = useState<IntruderSnapshotViewModel[]>([
    { id: 1, time: '今天 10:42' },
    { id: 2, time: '昨天 15:20' },
  ]);

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
        const backendTriggerMode = logonWakeModeToTriggerMode(
          response.safe_details.logon_wake_mode,
        );
        if (backendTriggerMode) {
          setTriggerMode(backendTriggerMode);
        }
      })
      .catch((error) => {
        console.warn('Failed to load WinFaceUnlock settings.', error);
      });

    loadFaceTemplates();

    return () => {
      isMounted = false;
    };
  }, [loadFaceTemplates]);

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
      if (nextMode !== 'keyboard') {
        if (!isControlRuntimeAvailable()) {
          setTriggerMode(nextMode);
        } else {
          console.warn('Logon wake mode is not implemented yet.', nextMode);
        }
        return;
      }

      const previousMode = triggerMode;
      setTriggerMode(nextMode);

      if (!isControlRuntimeAvailable()) {
        return;
      }

      const requestId = triggerModeRequestId.current + 1;
      triggerModeRequestId.current = requestId;

      updateControlSettings(controlTransport, { logon_wake_mode: 'input_triggered' })
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

  return {
    autoLock,
    intruderSnap,
    triggerMode,
    enrolledFaces,
    intruders,
    setIntruderSnap,
    changeAutoLock,
    changeTriggerMode,
    deleteFace,
  };
}

function faceTemplateToViewModel(template: FaceTemplateSummary): EnrolledFaceViewModel {
  return {
    id: template.face_template_ref,
    name:
      template.display_name ??
      template.user_id ??
      `人脸 ${template.selected_template_count}`,
  };
}

function logonWakeModeToTriggerMode(mode?: LogonWakeMode): TriggerMode | undefined {
  if (mode === 'input_triggered') {
    return 'keyboard';
  }
  return undefined;
}
