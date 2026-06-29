import { motion, AnimatePresence } from 'motion/react';
import { Camera, Keyboard, ScanFace, ShieldAlert, UserCog, User, X } from 'lucide-react';
import { useSettingsArea, type TriggerMode } from '../bindings/useSettingsArea';

export function SettingsArea() {
  const {
    autoLock,
    intruderSnap,
    triggerMode,
    logonFaceMatchThreshold,
    enrolledFaces,
    intruders,
    setIntruderSnap,
    changeAutoLock: handleAutoLockChange,
    changeTriggerMode: handleTriggerModeChange,
    changeLogonFaceMatchThreshold: handleLogonFaceMatchThresholdChange,
    deleteFace: handleFaceDelete,
    deleteIntruder,
  } = useSettingsArea();

  return (
    <div className="relative z-50 flex min-h-0 flex-1 flex-col items-center justify-start gap-8 overflow-y-auto overscroll-contain px-6 pt-8 pb-20 w-full max-w-2xl mx-auto scrollbar-hide">
      
      {/* 核心安全控制 */}
      <motion.div 
        initial={{ y: 20, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 200, damping: 20 }}
        className="w-full flex flex-col gap-2"
      >
        <h3 className="text-sm font-medium text-slate-500 px-4">安全控制</h3>
        <div className="bg-white/70 backdrop-blur-md border border-slate-200/80 rounded-2xl overflow-hidden shadow-sm flex flex-col">
          
          {/* 离座落锁 */}
          <div className="flex items-center justify-between p-4 border-b border-slate-100 last:border-0 hover:bg-white/40 transition-colors">
             <div className="flex items-center gap-4">
                <div className="p-2 bg-blue-50 text-[#007acc] rounded-xl">
                   <UserCog className="w-5 h-5" />
                </div>
                <div className="flex flex-col">
                   <span className="text-base font-medium text-slate-800">离座落锁</span>
                   <span className="text-xs text-slate-500">检测到离开电脑前时自动锁定屏幕</span>
                </div>
             </div>
             <Switch checked={autoLock} onChange={handleAutoLockChange} />
          </div>

          {/* 入侵者抓拍 */}
          <div className="flex flex-col border-b border-slate-100 last:border-0 hover:bg-white/40 transition-colors">
            <div className="flex items-center justify-between p-4">
               <div className="flex items-center gap-4">
                  <div className="p-2 bg-red-50 text-red-500 rounded-xl">
                     <ShieldAlert className="w-5 h-5" />
                  </div>
                  <div className="flex flex-col">
                     <span className="text-base font-medium text-slate-800">防偷窥 / 入侵者抓拍</span>
                     <span className="text-xs text-slate-500">他人试图解锁或偷看屏幕时自动拍下照片</span>
                  </div>
               </div>
               <Switch checked={intruderSnap} onChange={setIntruderSnap} />
            </div>

            <AnimatePresence>
              {intruderSnap && intruders.length > 0 && (
                <motion.div 
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: 'auto', opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  className="px-[60px] pb-4 overflow-hidden"
                >
                  <div className="flex gap-4 overflow-x-auto pb-2 scrollbar-hide">
                    {intruders.map(intruder => (
                      <div key={intruder.id} className="relative flex flex-col items-center gap-2 group px-1 pt-1">
                        <div className="relative h-12 w-12 rounded-full border-2 border-red-100 bg-red-50 flex items-center justify-center overflow-hidden">
                          {intruder.avatarSrc ? (
                            <img
                              src={intruder.avatarSrc}
                              alt=""
                              className="h-full w-full object-cover"
                            />
                          ) : (
                            <User className="h-5 w-5 text-red-400" />
                          )}
                        </div>
                        <button 
                          onClick={() => deleteIntruder(intruder.id)}
                          className="absolute top-0 right-0 bg-white rounded-full p-0.5 shadow-sm border border-red-200 text-slate-400 hover:text-red-500 hover:bg-red-50 transition-colors z-10 opacity-0 group-hover:opacity-100"
                        >
                          <X className="h-3 w-3" />
                        </button>
                        <span className="text-[10px] text-slate-400 font-medium">{intruder.time}</span>
                      </div>
                    ))}
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
          
        </div>
      </motion.div>

      {/* 识别设置 */}
      <motion.div 
        initial={{ y: 20, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 200, damping: 20, delay: 0.1 }}
        className="w-full flex flex-col gap-2"
      >
        <h3 className="text-sm font-medium text-slate-500 px-4">识别与管理</h3>
        <div className="bg-white/70 backdrop-blur-md border border-slate-200/80 rounded-2xl overflow-hidden shadow-sm flex flex-col">
          
          {/* 人脸管理 */}
          <div className="flex flex-col border-b border-slate-100 last:border-0 hover:bg-white/40 transition-colors">
            <div className="flex items-center justify-between p-4">
               <div className="flex items-center gap-4">
                  <div className="p-2 bg-purple-50 text-purple-600 rounded-xl">
                     <ScanFace className="w-5 h-5" />
                  </div>
                  <div className="flex flex-col">
                     <span className="text-base font-medium text-slate-800">人脸管理</span>
                     <span className="text-xs text-slate-500">录入、更新或移除授权登入的人脸信息</span>
                  </div>
               </div>
            </div>

            <div className="px-[60px] pb-4">
              <div className="flex gap-4 overflow-x-auto pt-1 pr-1 pb-2 scrollbar-hide items-start">
                {enrolledFaces.map(face => (
                  <div key={face.id} className="relative flex flex-col items-center gap-2 group px-1 pt-1">
                    <div className="relative h-12 w-12 rounded-full border border-slate-200 bg-slate-50 flex items-center justify-center overflow-hidden">
                      {face.avatarImageSrc ? (
                        <img
                          src={face.avatarImageSrc}
                          alt=""
                          className="h-full w-full object-cover"
                        />
                      ) : (
                        <User className="h-5 w-5 text-slate-400" />
                      )}
                    </div>
                    <button 
                      onClick={() => handleFaceDelete(face.id)}
                      className="absolute top-0 right-0 bg-white rounded-full p-0.5 shadow-sm border border-slate-200 text-slate-400 hover:text-red-500 hover:border-red-200 transition-colors z-10 opacity-0 group-hover:opacity-100"
                    >
                      <X className="h-3 w-3" />
                    </button>
                    <span className="text-xs text-slate-600 font-medium">{face.name}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>

          {/* 触发方式 */}
          <div className="flex flex-col p-4 border-b border-slate-100 last:border-0 hover:bg-white/40 transition-colors gap-4">
             <div className="flex flex-col">
                <span className="text-base font-medium text-slate-800">人脸识别触发方式</span>
                <span className="text-xs text-slate-500">选择适合你使用习惯的面部扫描唤醒方式</span>
             </div>
             
             <div className="flex p-1 bg-slate-100/50 rounded-xl border border-slate-200/50 relative">
               {[
                 { id: 'keyboard', label: '敲击键盘', Icon: Keyboard, disabled: false },
                 { id: 'silent', label: '后台静默', Icon: Camera, disabled: false },
               ].map(({ id, label, Icon, disabled }) => (
                 <button
                   key={id}
                   onClick={() => !disabled && handleTriggerModeChange(id as TriggerMode)}
                   disabled={disabled}
                   aria-disabled={disabled}
                   title={disabled ? '暂未开放' : undefined}
                   className={`relative flex-1 py-2 rounded-lg text-sm font-medium transition-colors flex items-center justify-center gap-1.5 outline-none ${
                     triggerMode === id ? 'text-white' : disabled ? 'text-slate-400 cursor-not-allowed opacity-60' : 'text-slate-500 hover:text-slate-700'
                   }`}
                 >
                   {triggerMode === id && (
                     <motion.div
                       layoutId="activeTriggerMode"
                       className="absolute inset-0 bg-[#007acc] rounded-lg shadow-sm"
                       transition={{ type: "spring", stiffness: 300, damping: 25 }}
                     />
                   )}
                   <span className="relative z-10 flex items-center gap-1.5">
                     <Icon className="w-4 h-4" />
                     {label}
                   </span>
                 </button>
               ))}
             </div>
          </div>

          {/* 匹配阈值 */}
          <div className="flex flex-col p-4 border-b border-slate-100 last:border-0 hover:bg-white/40 transition-colors gap-4">
             <div className="flex items-center justify-between gap-4">
                <div className="flex flex-col">
                   <span className="text-base font-medium text-slate-800">登录匹配阈值</span>
                   <span className="text-xs text-slate-500">调低更容易通过，调高更严格</span>
                </div>
                <span className="min-w-14 rounded-lg border border-slate-200 bg-white/70 px-2 py-1 text-center text-sm font-semibold text-slate-700">
                  {logonFaceMatchThreshold.toFixed(2)}
                </span>
             </div>

             <div className="flex flex-col gap-2">
               <input
                 type="range"
                 min="0.30"
                 max="0.90"
                 step="0.01"
                 value={logonFaceMatchThreshold}
                 aria-label="登录匹配阈值"
                 onChange={(event) => {
                   let val = Number(event.currentTarget.value);
                   handleLogonFaceMatchThresholdChange(val);
                 }}
                 className="h-2 w-full cursor-pointer accent-[#007acc]"
               />
               <div className="relative h-4 text-[11px] font-medium text-slate-400 mt-1">
                 <span className="absolute left-0">宽松 0.30</span>
                 <span className="absolute left-[75%] -translate-x-1/2">默认 0.75</span>
                 <span className="absolute right-0">严格 0.90</span>
               </div>
             </div>
          </div>
          
        </div>
      </motion.div>

    </div>
  );
}

function Switch({ checked, onChange }: { checked: boolean, onChange: (c: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors outline-none focus:outline-none ${
        checked ? 'bg-[#007acc]' : 'bg-slate-300'
      }`}
    >
      <span
        className={`inline-block h-5 w-5 transform rounded-full bg-white shadow-sm transition-transform ${
          checked ? 'translate-x-5' : 'translate-x-1'
        }`}
      />
    </button>
  );
}
