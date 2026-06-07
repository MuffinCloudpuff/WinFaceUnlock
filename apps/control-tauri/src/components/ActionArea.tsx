import { Database, RefreshCw, ScanFace, Server, Settings2, ShieldCheck } from 'lucide-react';
import { motion, AnimatePresence } from 'motion/react';
import { useState, useRef, useEffect } from 'react';

export type DashboardTone = 'good' | 'warn' | 'bad' | 'neutral';

export interface DashboardViewItem {
  id: 'service' | 'provider' | 'config' | 'data';
  title: string;
  value: string;
  detail: string;
  tone: DashboardTone;
}

export interface DashboardViewState {
  connectionState: 'loading' | 'connected' | 'error';
  message: string;
  items: DashboardViewItem[];
}

interface ActionAreaProps {
  dashboard: DashboardViewState;
  isRefreshingDashboard: boolean;
  onRefreshDashboard: () => void;
}

export function ActionArea({
  dashboard,
  isRefreshingDashboard,
  onRefreshDashboard,
}: ActionAreaProps) {
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
    <div className="relative z-50 flex min-h-0 flex-1 flex-col px-5 pb-5 pt-3">
      <StatusStrip
        dashboard={dashboard}
        isRefreshingDashboard={isRefreshingDashboard}
        onRefreshDashboard={onRefreshDashboard}
      />

      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-8 sm:gap-12">
        <AnimatePresence mode="wait">
          {!isRecording ? (
            <motion.div
              key="icon"
              initial={{ scale: 0.9, opacity: 0, filter: 'blur(10px)', rotateY: 0 }}
              animate={{ scale: 1, opacity: 1, filter: 'blur(0px)', rotateY: 0 }}
              exit={{ scale: 0.8, opacity: 0, rotateY: 90, filter: 'blur(10px)' }}
              transition={{ type: "spring", stiffness: 150, damping: 20 }}
              className="relative flex h-28 w-28 items-center justify-center sm:h-36 sm:w-36"
            >
              <div className="absolute inset-0 rounded-full bg-[#0066b8]/10 blur-2xl" />
              <ScanFace
                className="relative z-10 h-20 w-20 text-[#0066b8] drop-shadow-md sm:h-24 sm:w-24"
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
              className="relative flex h-44 w-44 items-center justify-center overflow-hidden rounded-full bg-slate-100 p-1 shadow-xl sm:h-56 sm:w-56"
            >
              <div
                className="absolute inset-0 animate-spin"
                style={{
                  backgroundImage: 'conic-gradient(from 0deg, transparent 0 120deg, #007acc 180deg, transparent 180deg 300deg, #007acc 360deg)',
                  animationDuration: '4s',
                  animationTimingFunction: 'linear'
                }}
              />

              <div className="relative z-10 h-full w-full overflow-hidden rounded-full bg-black">
                <video
                  ref={videoRef}
                  autoPlay
                  playsInline
                  muted
                  className="h-full w-full scale-x-[-1] object-cover"
                />
                <motion.div
                  animate={{ y: ["-10%", "150%"] }}
                  transition={{ duration: 2, repeat: Infinity, ease: "linear" }}
                  className="absolute left-0 right-0 top-0 h-1/2 border-b-2 border-[#007acc]/80 bg-gradient-to-b from-transparent to-[#007acc]/30"
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
              className="w-full max-w-72 rounded-full bg-[#0066b8] px-8 py-3 text-base font-normal text-white shadow-lg shadow-[#0066b8]/20 transition-all hover:bg-[#005a9e] focus:outline-none active:bg-[#004c87]"
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
              className="w-full max-w-72 rounded-full bg-slate-200 px-8 py-3 text-base font-normal text-slate-700 shadow-md transition-all hover:bg-slate-300 focus:outline-none active:bg-slate-400"
            >
              取消录入
            </motion.button>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

function StatusStrip({
  dashboard,
  isRefreshingDashboard,
  onRefreshDashboard,
}: ActionAreaProps) {
  return (
    <section className="shrink-0">
      <div className="mb-2 flex items-center justify-between gap-3">
        <div className="min-w-0 text-xs text-slate-500">
          <span className={connectionDotClass(dashboard.connectionState)} />
          <span className="align-middle">{dashboard.message}</span>
        </div>
        <button
          type="button"
          onClick={onRefreshDashboard}
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-white/70 bg-white/70 text-slate-600 shadow-sm backdrop-blur-xl transition hover:bg-white focus:outline-none"
          title="刷新状态"
          aria-label="刷新状态"
        >
          <RefreshCw className={`h-4 w-4 ${isRefreshingDashboard ? 'animate-spin' : ''}`} />
        </button>
      </div>
      <div className="grid grid-cols-4 gap-2">
        {dashboard.items.map((item) => (
          <div
            key={item.id}
            className="min-h-16 rounded-lg border border-white/70 bg-white/70 px-3 py-2 shadow-sm backdrop-blur-xl"
          >
            <div className="mb-1 flex items-center justify-between gap-2">
              <div className="flex min-w-0 items-center gap-1.5 text-xs text-slate-500">
                <StatusIcon id={item.id} />
                <span className="truncate">{item.title}</span>
              </div>
              <span className={`h-2 w-2 shrink-0 rounded-full ${toneDotClass(item.tone)}`} />
            </div>
            <div className="truncate text-sm font-semibold text-slate-800">{item.value}</div>
            <div className="truncate text-[11px] text-slate-500">{item.detail}</div>
          </div>
        ))}
      </div>
    </section>
  );
}

function StatusIcon({ id }: { id: DashboardViewItem['id'] }) {
  const className = "h-3.5 w-3.5 shrink-0";
  switch (id) {
    case 'service':
      return <Server className={className} />;
    case 'provider':
      return <ShieldCheck className={className} />;
    case 'config':
      return <Settings2 className={className} />;
    case 'data':
      return <Database className={className} />;
  }
}

function connectionDotClass(state: DashboardViewState['connectionState']) {
  const base = 'mr-2 inline-block h-2 w-2 rounded-full align-middle';
  if (state === 'connected') {
    return `${base} bg-emerald-500`;
  }
  if (state === 'error') {
    return `${base} bg-rose-500`;
  }
  return `${base} bg-amber-400`;
}

function toneDotClass(tone: DashboardTone) {
  switch (tone) {
    case 'good':
      return 'bg-emerald-500';
    case 'warn':
      return 'bg-amber-400';
    case 'bad':
      return 'bg-rose-500';
    default:
      return 'bg-slate-300';
  }
}
