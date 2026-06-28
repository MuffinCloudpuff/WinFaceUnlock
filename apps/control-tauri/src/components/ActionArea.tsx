import { ScanFace, ChevronLeft, ChevronRight } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type FaceEnrollmentViewModel } from '../bindings/useFaceEnrollment';

interface ActionAreaProps {
  enrollment: FaceEnrollmentViewModel;
}

export function ActionArea({ enrollment }: ActionAreaProps) {
  const status = enrollment.displayState;

  const handleSwitchCamera = (direction: 'next' | 'prev') => {
    if (enrollment.cameras.length <= 1) return;
    const currentIndex = enrollment.cameras.findIndex(
      (c) => c.camera_id === enrollment.selectedCameraId
    );
    let newIndex = currentIndex >= 0 ? currentIndex : 0;
    if (direction === 'next') {
      newIndex = (newIndex + 1) % enrollment.cameras.length;
    } else {
      newIndex = (newIndex - 1 + enrollment.cameras.length) % enrollment.cameras.length;
    }
    enrollment.switchCamera(enrollment.cameras[newIndex].camera_id);
  };

  return (
    <div className="relative z-50 flex flex-1 flex-col items-center justify-center gap-28">
      <AnimatePresence mode="wait">
        {status === 'idle' && (
          <motion.div
            key="icon"
            initial={{ scale: 0.9, opacity: 0, filter: 'blur(10px)', rotateY: 0 }}
            animate={{ scale: 1, opacity: 1, filter: 'blur(0px)', rotateY: 0 }}
            exit={{ scale: 0.8, opacity: 0, rotateY: 90, filter: 'blur(10px)' }}
            transition={{ type: 'spring', stiffness: 150, damping: 20 }}
            className="relative flex h-40 w-40 items-center justify-center"
          >
            <div className="absolute inset-0 rounded-full bg-[#0066b8]/10 blur-2xl" />
            <ScanFace
              className="relative z-10 h-28 w-28 text-[#0066b8] drop-shadow-md"
              strokeWidth={1.5}
            />
          </motion.div>
        )}

        {status === 'recording' && (
          <motion.div
            key="camera"
            initial={{ scale: 0.8, opacity: 0, rotateY: -90 }}
            animate={{ scale: 1, opacity: 1, rotateY: 0 }}
            exit={{ scale: 0.8, opacity: 0, rotateY: 90 }}
            transition={{ type: 'spring', stiffness: 150, damping: 20 }}
            className="flex flex-col items-center gap-8"
          >
            <div className="flex items-center justify-center gap-6">
              {enrollment.cameras.length > 1 && (
                <button
                  type="button"
                  onClick={() => handleSwitchCamera('prev')}
                  disabled={enrollment.isCommandPending}
                  className="rounded-full bg-white p-3 text-slate-400 shadow-md transition-all hover:bg-slate-50 hover:text-[#0066b8] active:scale-95 disabled:opacity-50"
                  aria-label="上一个摄像头"
                >
                  <ChevronLeft className="h-6 w-6" />
                </button>
              )}
              
              <div className="relative flex h-64 w-64 items-center justify-center overflow-hidden rounded-full bg-slate-100 p-1 shadow-xl">
                <div
                  className="absolute inset-0 animate-spin"
                  style={{
                    backgroundImage:
                      'conic-gradient(from 0deg, transparent 0 120deg, #007acc 180deg, transparent 180deg 300deg, #007acc 360deg)',
                    animationDuration: '4s',
                    animationTimingFunction: 'linear',
                  }}
                />

                <div className="relative z-10 flex h-full w-full items-center justify-center overflow-hidden rounded-full bg-black">
                  {enrollment.previewImageSrc ? (
                    <img
                      src={enrollment.previewImageSrc}
                      alt=""
                      className="h-full w-full scale-x-[-1] object-cover"
                    />
                  ) : (
                    <ScanFace
                      className="h-20 w-20 text-white/35"
                      strokeWidth={1.5}
                    />
                  )}
                  <motion.div
                    animate={{ y: ['-10%', '150%'] }}
                    transition={{ duration: 2, repeat: Infinity, ease: 'linear' }}
                    className="absolute top-0 left-0 right-0 h-1/2 border-b-2 border-[#007acc]/80 bg-gradient-to-b from-transparent to-[#007acc]/30"
                  />
                </div>
              </div>

              {enrollment.cameras.length > 1 && (
                <button
                  type="button"
                  onClick={() => handleSwitchCamera('next')}
                  disabled={enrollment.isCommandPending}
                  className="rounded-full bg-white p-3 text-slate-400 shadow-md transition-all hover:bg-slate-50 hover:text-[#0066b8] active:scale-95 disabled:opacity-50"
                  aria-label="下一个摄像头"
                >
                  <ChevronRight className="h-6 w-6" />
                </button>
              )}
            </div>

            <div className="flex h-8 items-center justify-center gap-3">
              <AnimatePresence mode="wait">
                <motion.div
                  key={enrollment.instructionText}
                  initial={{ opacity: 0, y: 10 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -10 }}
                  className="text-lg font-bold tracking-widest text-slate-800"
                >
                  {enrollment.instructionText}
                </motion.div>
              </AnimatePresence>
              {enrollment.progressText && (
                <span className="rounded-full bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-500">
                  {enrollment.progressText}
                </span>
              )}
            </div>
          </motion.div>
        )}

        {status === 'success' && (
          <motion.div
            key="success"
            initial={{ scale: 0.8, opacity: 0, rotateY: -90 }}
            animate={{ scale: 1, opacity: 1, rotateY: 0 }}
            exit={{ scale: 0.8, opacity: 0, filter: 'blur(10px)' }}
            transition={{ type: 'spring', stiffness: 150, damping: 20 }}
            className="relative flex h-40 w-40 items-center justify-center rounded-full border border-green-100 bg-white shadow-xl"
          >
            <div className="absolute inset-0 rounded-full bg-green-500/10 blur-xl" />
            <svg
              className="relative z-10 h-20 w-20 text-green-500"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={3}
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <motion.path
                initial={{ pathLength: 0 }}
                animate={{ pathLength: 1 }}
                transition={{ duration: 0.6, ease: 'easeOut', delay: 0.1 }}
                d="M5 13l4 4L19 7"
              />
            </svg>
          </motion.div>
        )}

        {status === 'failure' && (
          <motion.div
            key="failure"
            initial={{ scale: 0.8, opacity: 0, rotateY: -90 }}
            animate={{ scale: 1, opacity: 1, rotateY: 0 }}
            exit={{ scale: 0.8, opacity: 0, filter: 'blur(10px)' }}
            transition={{ type: 'spring', stiffness: 150, damping: 20 }}
            className="relative flex h-40 w-40 items-center justify-center rounded-full border border-red-100 bg-white shadow-xl"
          >
            <div className="absolute inset-0 rounded-full bg-red-500/10 blur-xl" />
            <svg
              className="relative z-10 h-20 w-20 text-red-500"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={3}
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <motion.path
                initial={{ pathLength: 0 }}
                animate={{ pathLength: 1 }}
                transition={{ duration: 0.6, ease: 'easeOut', delay: 0.1 }}
                d="M6 18L18 6M6 6l12 12"
              />
            </svg>
          </motion.div>
        )}

      </AnimatePresence>

      <AnimatePresence mode="popLayout">
        {status === 'idle' && (
          <motion.div
            key="start-controls"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            className="flex flex-col items-center gap-3"
          >

            <motion.button
              whileHover={{ scale: 1.02 }}
              whileTap={{ scale: 0.97 }}
              onClick={enrollment.startEnrollment}
              disabled={enrollment.isCommandPending}
              className="rounded-full bg-[#0066b8] px-32 py-3 text-base font-bold tracking-wider text-white shadow-lg shadow-[#0066b8]/20 transition-all hover:bg-[#005a9e] focus:outline-none active:bg-[#004c87]"
            >
              开始录入
            </motion.button>
          </motion.div>
        )}

        {status === 'recording' && (
          <motion.button
            key="cancel-btn"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.97 }}
            onClick={enrollment.cancelEnrollment}
            disabled={enrollment.isCommandPending}
            className="rounded-full bg-slate-200 px-32 py-3 text-base font-bold tracking-wider text-slate-700 shadow-md transition-all hover:bg-slate-300 focus:outline-none active:bg-slate-400"
          >
            取消录入
          </motion.button>
        )}

        {status === 'success' && (
          <motion.div
            key="success-text"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            className="px-32 py-3 text-base font-bold tracking-wider text-green-600"
          >
            录入成功
          </motion.div>
        )}

        {status === 'failure' && (
          <motion.div
            key="failure-text"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            className="flex w-full max-w-md flex-col items-center gap-3 px-6 text-center"
          >
            <div className="text-base font-bold tracking-wider text-red-600">
              {enrollment.message ?? '录入失败'}
            </div>
            {enrollment.nextRecommendedAction && (
              <div className="text-sm font-medium leading-6 text-slate-500">
                {enrollment.nextRecommendedAction}
              </div>
            )}
            <button
              type="button"
              onClick={enrollment.resetEnrollment}
              className="rounded-full bg-slate-200 px-12 py-3 text-sm font-bold tracking-wider text-slate-700 shadow-md transition-all hover:bg-slate-300 focus:outline-none active:bg-slate-400"
            >
              重新选择摄像头
            </button>
          </motion.div>
        )}

      </AnimatePresence>
    </div>
  );
}
