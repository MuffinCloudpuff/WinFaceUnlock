import { User, KeyRound, ArrowRight } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useState } from 'react';
import { isControlRuntimeAvailable } from '../bindings/controlTransport';
import { useCredentialEnrollment } from '../bindings/useCredentialEnrollment';

export function AccountArea() {
  const [pin, setPin] = useState('');
  const [isEditingCredential, setIsEditingCredential] = useState(false);
  const [showSuccessAnimation, setShowSuccessAnimation] = useState(false);
  const { accountProfile, credentialEnrollmentCompletedAt, isSubmitting, submitCredential } =
    useCredentialEnrollment();
  const credentialConfigured = accountProfile?.credential_secret_state === 'configured';
  const shouldShowCredentialInput = !credentialConfigured || isEditingCredential;
  const accountDisplayName =
    accountProfile?.display_name ??
    accountProfile?.windows_account_username ??
    '用户1';

  useEffect(() => {
    if (credentialEnrollmentCompletedAt === null) {
      return;
    }

    setIsEditingCredential(false);
    setShowSuccessAnimation(true);
  }, [credentialEnrollmentCompletedAt]);

  useEffect(() => {
    if (!showSuccessAnimation) {
      return;
    }
    const timer = window.setTimeout(() => {
      setShowSuccessAnimation(false);
    }, 1200);

    return () => window.clearTimeout(timer);
  }, [showSuccessAnimation]);

  const handleCredentialSubmit = () => {
    if (pin.length === 0 || isSubmitting) {
      return;
    }

    const passwordSecret = pin;
    setPin('');
    submitCredential(passwordSecret);
  };

  return (
    <div className="relative z-50 flex flex-1 flex-col items-center justify-center gap-10">
      <motion.div
        initial={{ y: 20, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 200, damping: 20 }}
        className="flex flex-col items-center gap-4"
      >
        <div className="flex h-24 w-24 items-center justify-center rounded-full bg-white shadow-sm border border-slate-200/60 overflow-hidden relative">
          {/* A subtle glowing presence behind the avatar */}
          <div className="absolute inset-0 bg-blue-50/50" />
          <User className="relative z-10 h-10 w-10 text-slate-400" strokeWidth={1.5} />
        </div>
        <h2 className="text-xl font-medium text-slate-800 tracking-tight">
          {accountDisplayName}
        </h2>
      </motion.div>

      <motion.div
        initial={{ y: 20, opacity: 0, filter: 'blur(5px)' }}
        animate={{ y: 0, opacity: 1, filter: 'blur(0px)' }}
        transition={{ type: "spring", stiffness: 200, damping: 20, delay: 0.1 }}
        className="w-full max-w-xs flex flex-col gap-4 h-[60px]"
      >
        <AnimatePresence mode="wait">
          {shouldShowCredentialInput || showSuccessAnimation ? (
            <motion.div
              key="input-container"
              layout
              initial={{ width: '100%', borderRadius: 12, backgroundColor: 'transparent' }}
              animate={{
                width: showSuccessAnimation ? 60 : '100%',
                borderRadius: showSuccessAnimation ? 30 : 12,
                backgroundColor: 'transparent',
              }}
              transition={{ type: 'spring', stiffness: 350, damping: 30 }}
              className="relative mx-auto h-[60px] flex items-center justify-center overflow-hidden"
              style={{ padding: showSuccessAnimation ? 0 : 2 }}
            >
              {!showSuccessAnimation && (
                <motion.div
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0, scale: 0.8 }}
                  transition={{ duration: 0.15 }}
                  className="w-full h-full relative group shadow-sm rounded-xl"
                >
                  <div className="absolute inset-0 bg-slate-200/80 rounded-[10px] group-focus-within:hidden transition-all" />
                  <div
                    className="absolute top-1/2 left-1/2 w-[300%] aspect-square -translate-x-1/2 -translate-y-1/2 hidden group-focus-within:block animate-spin"
                    style={{
                      backgroundImage:
                        'conic-gradient(from 0deg, transparent 0 120deg, #007acc 180deg, transparent 180deg 300deg, #007acc 360deg)',
                      animationDuration: '4s',
                      animationTimingFunction: 'linear',
                    }}
                  />
                  <div className="relative bg-white flex items-center rounded-[10px] w-[calc(100%-2px)] h-[calc(100%-2px)] overflow-hidden m-[1px]">
                    <div className="absolute inset-y-0 left-0 pl-3.5 flex items-center pointer-events-none z-10">
                      <KeyRound className="h-4 w-4 text-slate-400 group-focus-within:text-[#007acc] transition-colors" />
                    </div>
                    <input
                      type="password"
                      value={pin}
                      onChange={(e) => setPin(e.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter') {
                          event.preventDefault();
                          handleCredentialSubmit();
                        }
                      }}
                      className="block w-full h-full bg-transparent pl-10 pr-10 text-slate-800 placeholder-slate-400 focus:outline-none transition-all font-mono tracking-[0.3em] text-center relative z-0"
                      placeholder="PIN"
                      autoFocus
                    />
                    {pin.length > 0 && (
                      <motion.button
                        initial={{ scale: 0.8, opacity: 0 }}
                        animate={{ scale: 1, opacity: 1 }}
                        className="absolute inset-y-0 right-1.5 flex items-center z-10"
                        disabled={isSubmitting || (isControlRuntimeAvailable() && !accountProfile)}
                        onClick={handleCredentialSubmit}
                      >
                        <div className="bg-[#007acc] hover:bg-[#0066aa] active:scale-95 text-white p-2 rounded-lg transition-all shadow-sm">
                          <ArrowRight className="h-5 w-5" />
                        </div>
                      </motion.button>
                    )}
                  </div>
                </motion.div>
              )}
              {showSuccessAnimation && (
                <motion.svg
                  initial={{ opacity: 0, scale: 0.5 }}
                  animate={{ opacity: 1, scale: 1 }}
                  transition={{ delay: 0.1, type: 'spring', damping: 15 }}
                  className="w-12 h-12 relative left-0.5"
                  fill="none"
                  viewBox="0 0 24 24"
                  strokeWidth={5}
                  strokeLinecap="square"
                >
                  <motion.path
                    initial={{ pathLength: 0 }}
                    animate={{ pathLength: 1 }}
                    transition={{ duration: 0.2, ease: 'easeOut', delay: 0.1 }}
                    stroke="#1d5bbf"
                    d="M6.5 12.5l4 4"
                  />
                  <motion.path
                    initial={{ pathLength: 0 }}
                    animate={{ pathLength: 1 }}
                    transition={{ duration: 0.4, ease: 'easeOut', delay: 0.3 }}
                    stroke="#3fa0f0"
                    d="M10.5 16.5l7.5-8.5"
                  />
                </motion.svg>
              )}
            </motion.div>
          ) : (
            <motion.div
              key="saved"
              initial={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95 }}
              className="flex items-center justify-between p-3 bg-white/70 backdrop-blur-md border border-slate-200/80 rounded-xl shadow-sm w-full"
            >
              <div className="flex items-center gap-2 pl-2">
                <KeyRound className="h-4 w-4 text-slate-500" />
                <span className="text-sm font-medium text-slate-800">密码已设置</span>
              </div>
              <button
                onClick={() => {
                  setPin('');
                  setIsEditingCredential(true);
                }}
                className="px-4 py-1.5 mr-1 text-xs font-medium text-slate-700 bg-slate-100 hover:bg-slate-200 active:scale-95 rounded-lg transition-all"
              >
                修改
              </button>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}
