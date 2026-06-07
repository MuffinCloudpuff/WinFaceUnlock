import { Home, User, Settings } from 'lucide-react';
import { motion } from 'motion/react';

export type TabId = 'home' | 'account' | 'settings';

interface NavPillProps {
  activeTab: TabId;
  onTabChange: (tab: TabId) => void;
}

export function NavPill({ activeTab, onTabChange }: NavPillProps) {
  return (
    <div className="relative z-50 flex justify-center pt-2">
      <motion.div 
        initial={{ y: -10, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 300, damping: 30 }}
        className="flex items-center gap-0.5 rounded-full bg-white/60 p-1 shadow-sm backdrop-blur-xl border border-white/50"
      >
        <button 
          onClick={() => onTabChange('home')}
          className={`flex items-center gap-2 rounded-full px-6 py-2 text-sm font-medium transition-all ${
            activeTab === 'home' 
              ? 'bg-blue-50/90 text-[#0066b8]' 
              : 'text-slate-600 hover:bg-slate-100/50 hover:text-slate-900'
          }`}
        >
          <Home className="h-4 w-4" strokeWidth={activeTab === 'home' ? 2.5 : 2} />
          <span>首页</span>
        </button>
        
        <button 
          onClick={() => onTabChange('account')}
          className={`flex items-center gap-2 rounded-full px-6 py-2 text-sm font-medium transition-all ${
            activeTab === 'account' 
              ? 'bg-blue-50/90 text-[#0066b8]' 
              : 'text-slate-600 hover:bg-slate-100/50 hover:text-slate-900'
          }`}
        >
          <User className="h-4 w-4" strokeWidth={activeTab === 'account' ? 2.5 : 2} />
          <span>帐号</span>
        </button>
        
        <button 
          onClick={() => onTabChange('settings')}
          className={`flex items-center gap-2 rounded-full px-6 py-2 text-sm font-medium transition-all ${
            activeTab === 'settings' 
              ? 'bg-blue-50/90 text-[#0066b8]' 
              : 'text-slate-600 hover:bg-slate-100/50 hover:text-slate-900'
          }`}
        >
          <Settings className="h-4 w-4" strokeWidth={activeTab === 'settings' ? 2.5 : 2} />
          <span>设置</span>
        </button>
      </motion.div>
    </div>
  );
}
