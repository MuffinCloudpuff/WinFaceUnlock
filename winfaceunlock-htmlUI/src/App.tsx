import { useState } from 'react';
import { TitleBar } from './components/TitleBar';
import { NavPill, TabId } from './components/NavPill';
import { ActionArea } from './components/ActionArea';
import { AccountArea } from './components/AccountArea';
import { SettingsArea } from './components/SettingsArea';

export default function App() {
  const [activeTab, setActiveTab] = useState<TabId>('home');

  return (
    <div className="relative flex h-screen w-full flex-col overflow-hidden bg-[#fafbfe] font-sans selection:bg-blue-200">
      {/* Subtle Generative Fluid Art Background (Light mode aesthetic) */}
      <div className="pointer-events-none absolute inset-0 z-0 opacity-80 mix-blend-multiply">
        <div className="absolute -left-[10%] -top-[10%] h-[50vh] w-[50vw] rounded-full bg-[#e8f1f9] blur-[100px]" />
        <div className="absolute -right-[5%] top-[20%] h-[60vh] w-[40vw] rounded-full bg-[#f0f5fb] blur-[120px]" />
        <div className="absolute -bottom-[10%] left-[20%] h-[40vh] w-[50vw] rounded-full bg-[#e0eff8] blur-[110px]" />
      </div>

      <TitleBar />
      
      <div className="relative z-10 flex flex-1 flex-col">
        <NavPill activeTab={activeTab} onTabChange={setActiveTab} />
        {activeTab === 'home' && <ActionArea />}
        {activeTab === 'account' && <AccountArea />}
        {activeTab === 'settings' && <SettingsArea />}
      </div>


    </div>
  );
}
