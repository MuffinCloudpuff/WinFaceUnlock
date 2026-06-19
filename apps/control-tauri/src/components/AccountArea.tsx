import { User, KeyRound, ArrowRight } from 'lucide-react';
import { motion } from 'motion/react';
import { useState } from 'react';
import { isControlRuntimeAvailable } from '../bindings/controlTransport';
import { useCredentialEnrollment } from '../bindings/useCredentialEnrollment';

export function AccountArea() {
  const [pin, setPin] = useState('');
  const { accountProfile, isSubmitting, submitCredential } = useCredentialEnrollment();

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
          {accountProfile?.windows_account_username ?? 'Admin User'}
        </h2>
      </motion.div>

      <motion.div
        initial={{ y: 20, opacity: 0, filter: 'blur(5px)' }}
        animate={{ y: 0, opacity: 1, filter: 'blur(0px)' }}
        transition={{ type: "spring", stiffness: 200, damping: 20, delay: 0.1 }}
        className="w-full max-w-xs flex flex-col gap-4"
      >
        <div className="relative group p-[2px] rounded-xl overflow-hidden shadow-sm transition-all">
          {/* Static border background when not focused */}
          <div className="absolute inset-0 bg-slate-200/80 group-focus-within:hidden transition-all" />
          
          {/* Spinning gradient when focused */}
          <div 
            className="absolute top-1/2 left-1/2 w-[300%] aspect-square -translate-x-1/2 -translate-y-1/2 hidden group-focus-within:block animate-spin"
            style={{
              backgroundImage: 'conic-gradient(from 0deg, transparent 0 120deg, #007acc 180deg, transparent 180deg 300deg, #007acc 360deg)',
              animationDuration: '4s',
              animationTimingFunction: 'linear'
            }}
          />

          {/* Inner input container to cover the spinning background */}
          <div className="relative bg-white flex items-center rounded-[10px] w-full h-full overflow-hidden">
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
              className="block w-full bg-transparent pl-10 pr-10 py-3 text-slate-800 placeholder-slate-400 focus:outline-none transition-all font-mono tracking-[0.3em] text-center relative z-0"
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
                <div className="bg-[#007acc] hover:bg-[#0066aa] active:scale-95 text-white p-1.5 rounded-lg transition-all shadow-sm">
                  <ArrowRight className="h-4 w-4" />
                </div>
              </motion.button>
            )}
          </div>
        </div>

      </motion.div>
    </div>
  );
}
