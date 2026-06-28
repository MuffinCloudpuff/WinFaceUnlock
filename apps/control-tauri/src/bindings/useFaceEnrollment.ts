import {
  cancelFaceEnrollment,
  finishFaceEnrollment,
  listCameras,
  getFaceEnrollmentStatus,
  startFaceEnrollment,
  type CameraDeviceSummary,
  type ControlResponseEnvelope,
  type FaceEnrollmentSessionStatus,
} from '@winfaceunlock/control-client';
import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  controlTransport,
  isControlRuntimeAvailable,
} from './controlTransport';
import { notifyFaceTemplatesChanged } from './faceTemplateEvents';

export type FaceEnrollmentUiState =
  | 'idle'
  | 'starting'
  | 'running'
  | 'finishing'
  | 'completed'
  | 'failed'
  | 'cancelled';

export interface FaceEnrollmentViewModel {
  uiState: FaceEnrollmentUiState;
  sessionStatus: FaceEnrollmentSessionStatus | null;
  displayState: 'idle' | 'recording' | 'success' | 'failure';
  instructionText: string;
  progressText?: string;
  previewImageSrc?: string;
  message?: string;
  nextRecommendedAction?: string;
  cameras: CameraDeviceSummary[];
  selectedCameraId: string;
  setSelectedCameraId: (cameraId: string) => void;
  loadCameraList: () => void;
  isCameraListLoading: boolean;
  isCommandPending: boolean;
  startEnrollment: () => void;
  cancelEnrollment: () => void;
  switchCamera: (cameraId: string) => void;
  cancelAndResetEnrollment: () => void;
  resetEnrollment: () => void;
}

const STATUS_CHECK_DELAY_MS = 1500;
const FALLBACK_CAMERA_ID = 'opencv-index:0';
const CAMERA_SELECTION_STORAGE_KEY = 'winfaceunlock.selectedCameraId';
const FACE_ENROLLMENT_PREVIEW_EVENT = 'winfaceunlock://face-enrollment/preview-frame';

interface FaceEnrollmentPreviewEventPayload {
  enrollment_session_id: string;
  frame_seq: number;
  updated_at_unix_ms: number;
  mime_type: string;
  image_base64: string;
}

export function useFaceEnrollment(): FaceEnrollmentViewModel {
  const [uiState, setUiState] = useState<FaceEnrollmentUiState>('idle');
  const [sessionStatus, setSessionStatus] = useState<FaceEnrollmentSessionStatus | null>(null);
  const [message, setMessage] = useState<string>();
  const [nextRecommendedAction, setNextRecommendedAction] = useState<string>();
  const [cameras, setCameras] = useState<CameraDeviceSummary[]>([]);
  const [selectedCameraId, setSelectedCameraIdState] = useState(() => loadStoredCameraId());
  const [isCameraListLoading, setIsCameraListLoading] = useState(false);
  const [isCommandPending, setIsCommandPending] = useState(false);
  const [previewImageSrc, setPreviewImageSrc] = useState<string>();
  const finishingSessionIdRef = useRef<string | null>(null);

  const failFromResponse = useCallback((response: ControlResponseEnvelope<unknown>) => {
    setMessage(response.message);
    setNextRecommendedAction(response.next_recommended_action);
    setUiState('failed');
  }, []);

  const failFromError = useCallback((error: unknown, fallbackMessage: string) => {
    setMessage(error instanceof Error ? error.message : fallbackMessage);
    setNextRecommendedAction(undefined);
    setUiState('failed');
  }, []);

  const setSelectedCameraId = useCallback((cameraId: string) => {
    const nextCameraId = cameraId.trim() || FALLBACK_CAMERA_ID;
    setSelectedCameraIdState(nextCameraId);
    window.localStorage.setItem(CAMERA_SELECTION_STORAGE_KEY, nextCameraId);
  }, []);

  const finishCompletedEnrollment = useCallback(
    (enrollmentSessionId: string) => {
      if (finishingSessionIdRef.current === enrollmentSessionId) {
        return;
      }

      finishingSessionIdRef.current = enrollmentSessionId;
      setUiState('finishing');
      finishFaceEnrollment(controlTransport, {
        enrollment_session_id: enrollmentSessionId,
      })
        .then((finishResponse) => {
          if (finishingSessionIdRef.current !== enrollmentSessionId) {
            return;
          }

          if (finishResponse.operation_status !== 'completed') {
            failFromResponse(finishResponse);
            return;
          }

          setUiState('completed');
          setMessage(finishResponse.message);
          setNextRecommendedAction(finishResponse.next_recommended_action);
          notifyFaceTemplatesChanged();
        })
        .catch((error) => {
          if (finishingSessionIdRef.current === enrollmentSessionId) {
            failFromError(error, 'Failed to finish WinFaceUnlock face enrollment.');
          }
        })
        .finally(() => {
          if (finishingSessionIdRef.current === enrollmentSessionId) {
            finishingSessionIdRef.current = null;
          }
        });
    },
    [failFromError, failFromResponse],
  );

  const loadCameraList = useCallback(() => {
    if (!isControlRuntimeAvailable() || isCameraListLoading || cameras.length > 0) {
      return;
    }

    let isActive = true;
    setIsCameraListLoading(true);
    listCameras(controlTransport)
      .then((response) => {
        if (!isActive) {
          return;
        }
        if (response.operation_status !== 'completed') {
          setMessage(response.message);
          setNextRecommendedAction(response.next_recommended_action);
          return;
        }

        const nextCameras = response.safe_details.cameras;
        setCameras(nextCameras);
        if (
          nextCameras.length > 0 &&
          !nextCameras.some((camera) => camera.camera_id === selectedCameraId)
        ) {
          setSelectedCameraId(nextCameras[0].camera_id);
        }
      })
      .catch((error) => {
        if (isActive) {
          setMessage(error instanceof Error ? error.message : 'Failed to list cameras.');
          setNextRecommendedAction(undefined);
        }
      })
      .finally(() => {
        if (isActive) {
          setIsCameraListLoading(false);
        }
      });
  }, [cameras.length, isCameraListLoading, selectedCameraId, setSelectedCameraId]);

  useEffect(() => {
    loadCameraList();
  }, [loadCameraList]);

  useEffect(() => {
    if (!sessionStatus || !isControlRuntimeAvailable()) {
      return;
    }
    if (uiState !== 'running' && uiState !== 'starting') {
      return;
    }

    let isActive = true;
    let timeoutId: number | undefined;

    const checkEnrollmentStatus = () => {
      getFaceEnrollmentStatus(controlTransport, {
        enrollment_session_id: sessionStatus.enrollment_session_id,
      })
        .then((response) => {
          if (!isActive) {
            return;
          }

          if (response.operation_status !== 'completed') {
            failFromResponse(response);
            return;
          }

          const nextStatus = response.safe_details;
          setSessionStatus(nextStatus);

          if (nextStatus.session_state === 'completed') {
            finishCompletedEnrollment(nextStatus.enrollment_session_id);
            return;
          }

          if (nextStatus.session_state === 'failed') {
            setMessage(faceEnrollmentFailureText(nextStatus));
            setNextRecommendedAction(faceEnrollmentFailureAction(nextStatus));
            setUiState('failed');
            return;
          }

          if (nextStatus.session_state === 'cancelled') {
            setUiState('cancelled');
            return;
          }

          setUiState('running');
          timeoutId = window.setTimeout(checkEnrollmentStatus, STATUS_CHECK_DELAY_MS);
        })
        .catch((error) => {
          if (isActive) {
            failFromError(error, 'Failed to read WinFaceUnlock face enrollment status.');
          }
        });
    };

    timeoutId = window.setTimeout(checkEnrollmentStatus, STATUS_CHECK_DELAY_MS);

    return () => {
      isActive = false;
      if (timeoutId !== undefined) {
        window.clearTimeout(timeoutId);
      }
    };
  }, [failFromError, failFromResponse, finishCompletedEnrollment, sessionStatus, uiState]);

  useEffect(() => {
    if (!sessionStatus || !isControlRuntimeAvailable()) {
      return;
    }
    if (uiState !== 'running' && uiState !== 'starting') {
      return;
    }

    let isActive = true;
    let unlisten: (() => void) | undefined;

    listen<FaceEnrollmentPreviewEventPayload>(FACE_ENROLLMENT_PREVIEW_EVENT, (event) => {
      if (!isActive || event.payload.enrollment_session_id !== sessionStatus.enrollment_session_id) {
        return;
      }
      setPreviewImageSrc(
        `data:${event.payload.mime_type};base64,${event.payload.image_base64}`,
      );
    }).then((nextUnlisten) => {
      if (isActive) {
        unlisten = nextUnlisten;
      } else {
        nextUnlisten();
      }
    }).catch((error) => {
      if (isActive) {
        console.warn('Failed to subscribe to WinFaceUnlock face enrollment preview.', error);
      }
    });

    return () => {
      isActive = false;
      unlisten?.();
    };
  }, [sessionStatus, uiState]);

  const startEnrollment = useCallback(() => {
    if (isCommandPending) {
      return;
    }

    if (!isControlRuntimeAvailable()) {
      setMessage('Tauri 运行时未连接。');
      setNextRecommendedAction(undefined);
      setUiState('failed');
      return;
    }

    setMessage(undefined);
    setNextRecommendedAction(undefined);
    setPreviewImageSrc(undefined);
    setIsCommandPending(true);
    setUiState('starting');

    startFaceEnrollment(controlTransport, {
          camera_id: selectedCameraId,
        })
      .then((response) => {
        if (response.operation_status !== 'completed') {
          failFromResponse(response);
          return;
        }

        setSessionStatus(response.safe_details);
        setUiState('running');
      })
      .catch((error) => {
        failFromError(error, 'Failed to start WinFaceUnlock face enrollment.');
      })
      .finally(() => {
        setIsCommandPending(false);
      });
  }, [failFromError, failFromResponse, isCommandPending, selectedCameraId]);

  const cancelEnrollment = useCallback(() => {
    if (isCommandPending) {
      return;
    }

    if (!sessionStatus || !isControlRuntimeAvailable()) {
      setSessionStatus(null);
      setPreviewImageSrc(undefined);
      setUiState('idle');
      return;
    }

    setIsCommandPending(true);
    cancelFaceEnrollment(controlTransport, {
      enrollment_session_id: sessionStatus.enrollment_session_id,
    })
      .then((response) => {
        if (
          response.operation_status !== 'completed' &&
          response.operation_status !== 'cancelled'
        ) {
          failFromResponse(response);
          return;
        }

        setSessionStatus(response.safe_details);
        setUiState('cancelled');
      })
      .catch((error) => {
        failFromError(error, 'Failed to cancel WinFaceUnlock face enrollment.');
      })
      .finally(() => {
        setIsCommandPending(false);
      });
  }, [failFromError, failFromResponse, isCommandPending, sessionStatus]);

  const switchCamera = useCallback(
    (nextCameraId: string) => {
      if (isCommandPending || !sessionStatus || !isControlRuntimeAvailable()) {
        return;
      }
      setIsCommandPending(true);
      
      cancelFaceEnrollment(controlTransport, {
        enrollment_session_id: sessionStatus.enrollment_session_id,
      })
        .then(() => {
          setSelectedCameraId(nextCameraId);
          setPreviewImageSrc(undefined);
          setUiState('starting');
          return startFaceEnrollment(controlTransport, { camera_id: nextCameraId });
        })
        .then((response) => {
          if (response.operation_status !== 'completed') {
            failFromResponse(response);
            return;
          }
          setSessionStatus(response.safe_details);
          setUiState('running');
        })
        .catch((error) => {
          failFromError(error, 'Failed to switch camera.');
        })
        .finally(() => {
          setIsCommandPending(false);
        });
    },
    [failFromError, failFromResponse, isCommandPending, sessionStatus, setSelectedCameraId],
  );

  const resetEnrollment = useCallback(() => {
    finishingSessionIdRef.current = null;
    setSessionStatus(null);
    setMessage(undefined);
    setNextRecommendedAction(undefined);
    setPreviewImageSrc(undefined);
    setUiState('idle');
  }, []);

  const cancelAndResetEnrollment = useCallback(() => {
    finishingSessionIdRef.current = null;
    if (sessionStatus && isControlRuntimeAvailable()) {
      cancelFaceEnrollment(controlTransport, {
        enrollment_session_id: sessionStatus.enrollment_session_id,
      }).catch((error) => {
        console.warn('Failed to cancel WinFaceUnlock face enrollment during navigation.', error);
      });
    }
    setSessionStatus(null);
    setMessage(undefined);
    setNextRecommendedAction(undefined);
    setPreviewImageSrc(undefined);
    setUiState('idle');
  }, [sessionStatus]);

  const displayState = faceEnrollmentDisplayState(uiState);
  const instructionText = faceEnrollmentInstructionText(sessionStatus, message);
  const progressText = faceEnrollmentProgressText(sessionStatus);

  return {
    uiState,
    sessionStatus,
    displayState,
    instructionText,
    progressText,
    previewImageSrc,
    message,
    nextRecommendedAction,
    cameras,
    selectedCameraId,
    setSelectedCameraId,
    loadCameraList,
    isCameraListLoading,
    isCommandPending,
    startEnrollment,
    cancelEnrollment,
    switchCamera,
    cancelAndResetEnrollment,
    resetEnrollment,
  };
}

function loadStoredCameraId(): string {
  if (typeof window === 'undefined') {
    return FALLBACK_CAMERA_ID;
  }
  return window.localStorage.getItem(CAMERA_SELECTION_STORAGE_KEY) ?? FALLBACK_CAMERA_ID;
}

function faceEnrollmentDisplayState(
  uiState: FaceEnrollmentUiState,
): FaceEnrollmentViewModel['displayState'] {
  if (uiState === 'completed') {
    return 'success';
  }
  if (uiState === 'failed') {
    return 'failure';
  }
  if (uiState === 'starting' || uiState === 'running' || uiState === 'finishing') {
    return 'recording';
  }
  return 'idle';
}

function faceEnrollmentInstructionText(
  status: FaceEnrollmentSessionStatus | null,
  message?: string,
): string {
  if (!status) {
    return message ?? '请正脸看摄像头';
  }

  if (status.session_state === 'starting') {
    return '正在启动摄像头';
  }
  if (status.session_state === 'finishing') {
    return '正在保存人脸模板';
  }

  const instruction = status.current_instruction_code
    ? instructionTextByCode(status.current_instruction_code)
    : undefined;
  if (instruction) {
    return instruction;
  }

  if (status.current_step) {
    return instructionTextByStep(status.current_step) ?? '请保持面部在摄像头内';
  }

  return lastFrameInstructionText(status.last_frame_result) ?? message ?? '请正脸看摄像头';
}

function faceEnrollmentProgressText(status: FaceEnrollmentSessionStatus | null): string | undefined {
  if (!status?.required_sample_count) {
    return undefined;
  }
  return `${status.accepted_sample_count}/${status.required_sample_count}`;
}

function instructionTextByCode(code: string): string | undefined {
  const instructionByCode: Record<string, string> = {
    look_at_camera: '请正脸看摄像头',
    turn_head_left: '请缓慢向左转头',
    turn_head_right: '请缓慢向右转头',
    tilt_head_down: '请稍微低头',
    tilt_head_up: '请稍微抬头',
    blink_once: '请眨眼一次',
  };
  return instructionByCode[code];
}

function instructionTextByStep(step: string): string | undefined {
  const instructionByStep: Record<string, string> = {
    frontal_primary: '请正脸看摄像头',
    yaw_left_mild: '请缓慢向左转头',
    yaw_right_mild: '请缓慢向右转头',
    pitch_down_mild: '请稍微低头',
    pitch_up_mild: '请稍微抬头',
    blink_motion: '请眨眼一次',
  };
  return instructionByStep[step];
}

function lastFrameInstructionText(result?: string): string | undefined {
  if (result === 'no_face_detected') {
    return '请把面部移入摄像头范围';
  }
  if (result === 'multiple_faces_detected') {
    return '请确保画面中只有你一个人';
  }
  if (result === 'quality_rejected') {
    return '请保持光线稳定并看向摄像头';
  }
  if (result === 'model_unavailable') {
    return '人脸模型暂不可用';
  }
  return undefined;
}

function faceEnrollmentFailureText(status: FaceEnrollmentSessionStatus): string {
  const lastFrameText = lastFrameInstructionText(status.last_frame_result);
  if (lastFrameText) {
    return lastFrameText;
  }
  if (status.last_frame_result === 'pose_not_ready') {
    return '姿态未满足录入要求';
  }
  if (status.last_frame_result === 'face_accepted') {
    return '录入流程提前结束，未生成可用模板';
  }
  return '人脸录入失败';
}

function faceEnrollmentFailureAction(status: FaceEnrollmentSessionStatus): string | undefined {
  if (status.last_frame_result === 'no_face_detected') {
    return '确认选择的是正在使用的摄像头，并让面部完整出现在预览中。';
  }
  if (status.last_frame_result === 'pose_not_ready') {
    return '按提示正脸看摄像头，保持头部稳定后重试。';
  }
  if (status.last_frame_result === 'quality_rejected') {
    return '提高正面光照，避免背光、过暗或大幅晃动。';
  }
  if (status.last_frame_result === 'multiple_faces_detected') {
    return '保持画面里只有一个人后再重试。';
  }
  if (status.last_frame_result === 'model_unavailable') {
    return '检查本地人脸模型文件是否完整。';
  }
  return undefined;
}
