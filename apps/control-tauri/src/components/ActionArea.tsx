import { ScanFace } from 'lucide-react';
import { motion, AnimatePresence } from 'motion/react';
import { useState, useRef, useEffect } from 'react';

export function ActionArea() {
  const [isRecording, setIsRecording] = useState(false);
  const videoRef = useRef<HTMLVideoElement>(null);
  const streamRef = useRef<MediaStream | null>(null);

  useEffect(() => {
    if (isRecording) {
      startCamera();
    } else {
      stopCamera();
    }
    return () => stopCamera();
  }, [isRecording]);

  const startCamera = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ video: true });
      streamRef.current = stream;
      if (videoRef.current) {
        videoRef.current.srcObject = stream;
      }
    } catch (err) {
      console.error("Error accessing camera:", err);
    }
  };

  const stopCamera = () => {
    if (streamRef.current) {
      streamRef.current.getTracks().forEach(track => track.stop());
      streamRef.current = null;
    }
  };

  return (
    <div className="relative z-50 flex flex-1 flex-col items-center justify-center gap-28">
      <AnimatePresence mode="wait">
        {!isRecording ? (
          <motion.div
            key="icon"
            initial={{ scale: 0.9, opacity: 0, filter: 'blur(10px)', rotateY: 0 }}
            animate={{ scale: 1, opacity: 1, filter: 'blur(0px)', rotateY: 0 }}
            exit={{ scale: 0.8, opacity: 0, rotateY: 90, filter: 'blur(10px)' }}
            transition={{ type: "spring", stiffness: 150, damping: 20 }}
            className="relative flex h-40 w-40 items-center justify-center"
          >
            {/* A subtle glowing presence behind the key visual element */}
            <div className="absolute inset-0 rounded-full bg-[#0066b8]/10 blur-2xl" />
            <ScanFace
              className="relative z-10 h-28 w-28 text-[#0066b8] drop-shadow-md"
              strokeWidth={1.5}
            />
          </motion.div>
        ) : (
          <motion.div
            key="camera"
            initial={{ scale: 0.8, opacity: 0, rotateY: -90 }}
            animate={{ scale: 1, opacity: 1, rotateY: 0 }}
            exit={{ scale: 0.8, opacity: 0, rotateY: 90 }}
            transition={{ type: "spring", stiffness: 150, damping: 20 }}
            className="relative flex h-64 w-64 items-center justify-center rounded-full bg-slate-100 shadow-xl overflow-hidden p-1"
          >
            {/* Spinning scanning ring */}
            <div
              className="absolute inset-0 animate-spin"
              style={{
                backgroundImage: 'conic-gradient(from 0deg, transparent 0 120deg, #007acc 180deg, transparent 180deg 300deg, #007acc 360deg)',
                animationDuration: '4s',
                animationTimingFunction: 'linear'
              }}
            />

            <div className="relative h-full w-full rounded-full overflow-hidden bg-black z-10">
              <video
                ref={videoRef}
                autoPlay
                playsInline
                muted
                className="h-full w-full object-cover scale-x-[-1]" /* Mirror the video */
              />
              {/* Overlay scanning effect */}
              <motion.div
                animate={{ y: ["-10%", "150%"] }}
                transition={{ duration: 2, repeat: Infinity, ease: "linear" }}
                className="absolute top-0 left-0 right-0 h-1/2 bg-gradient-to-b from-transparent to-[#007acc]/30 border-b-2 border-[#007acc]/80"
              />
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence mode="popLayout">
        {!isRecording ? (
          <motion.button
            key="start-btn"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.97 }}
            onClick={() => setIsRecording(true)}
            className="rounded-full bg-[#0066b8] px-32 py-3 text-base font-normal tracking-wider text-white shadow-lg shadow-[#0066b8]/20 transition-all hover:bg-[#005a9e] focus:outline-none active:bg-[#004c87]"
          >
            开始录入
          </motion.button>
        ) : (
          <motion.button
            key="cancel-btn"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 20 }}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.97 }}
            onClick={() => setIsRecording(false)}
            className="rounded-full bg-slate-200 px-32 py-3 text-base font-normal tracking-wider text-slate-700 shadow-md transition-all hover:bg-slate-300 focus:outline-none active:bg-slate-400"
          >
            取消录入
          </motion.button>
        )}
      </AnimatePresence>
    </div>
  );
}
