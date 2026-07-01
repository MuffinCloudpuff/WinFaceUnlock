import {
  enrollWindowsCredential,
  getWindowsCredentialAccount,
  type WindowsCredentialAccountProfile,
} from '@winfaceunlock/control-client';
import { useCallback, useEffect, useState } from 'react';
import { controlTransport, isControlRuntimeAvailable } from './controlTransport';

export interface CredentialEnrollmentViewModel {
  accountProfile: WindowsCredentialAccountProfile | null;
  credentialEnrollmentCompletedAt: number | null;
  isSubmitting: boolean;
  message?: string;
  submitCredential: (passwordSecret: string) => void;
}

export function useCredentialEnrollment(): CredentialEnrollmentViewModel {
  const [accountProfile, setAccountProfile] =
    useState<WindowsCredentialAccountProfile | null>(null);
  const [credentialEnrollmentCompletedAt, setCredentialEnrollmentCompletedAt] = useState<number | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [message, setMessage] = useState<string>();

  useEffect(() => {
    if (!isControlRuntimeAvailable()) {
      return;
    }

    let isMounted = true;
    getWindowsCredentialAccount(controlTransport)
      .then((response) => {
        if (!isMounted) {
          return;
        }

        if (response.operation_status !== 'completed') {
          setMessage(response.message);
          return;
        }

        setAccountProfile(response.safe_details);
        setMessage(undefined);
      })
      .catch((error) => {
        if (isMounted) {
          setMessage(error instanceof Error ? error.message : 'Failed to load credential account.');
        }
      });

    return () => {
      isMounted = false;
    };
  }, []);

  const submitCredential = useCallback(
    (passwordSecret: string) => {
      if (passwordSecret.length === 0 || isSubmitting) {
        return;
      }

      if (!isControlRuntimeAvailable()) {
        setMessage('WinFaceUnlock credential enrollment requires the Tauri runtime.');
        return;
      }

      if (!accountProfile) {
        setMessage('WinFaceUnlock credential account is not loaded yet.');
        return;
      }

      setIsSubmitting(true);
      setMessage(undefined);
      setCredentialEnrollmentCompletedAt(null);
      enrollWindowsCredential(controlTransport, passwordSecret, accountProfile)
        .then((response) => {
          if (response.operation_status !== 'completed') {
            setMessage(response.message);
            return;
          }

          setAccountProfile(response.safe_details);
          setCredentialEnrollmentCompletedAt(Date.now());
          setMessage(undefined);
        })
        .catch((error) => {
          setMessage(error instanceof Error ? error.message : 'Failed to enroll credential.');
        })
        .finally(() => {
          setIsSubmitting(false);
        });
    },
    [accountProfile, isSubmitting],
  );

  return {
    accountProfile,
    credentialEnrollmentCompletedAt,
    isSubmitting,
    message,
    submitCredential,
  };
}
