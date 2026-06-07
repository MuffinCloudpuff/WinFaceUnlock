import { lazy, Suspense, useCallback, useEffect, useState } from 'react';
import { TitleBar } from './components/TitleBar';
import { NavPill, TabId } from './components/NavPill';
import { ActionArea, DashboardViewState } from './components/ActionArea';
import { DashboardStatus, loadDashboardStatus } from './controlProtocol';

const AccountArea = lazy(() =>
  import('./components/AccountArea').then((module) => ({ default: module.AccountArea })),
);
const SettingsArea = lazy(() =>
  import('./components/SettingsArea').then((module) => ({ default: module.SettingsArea })),
);

export default function App() {
  const [activeTab, setActiveTab] = useState<TabId>('home');
  const [dashboard, setDashboard] = useState<DashboardViewState>({
    connectionState: 'loading',
    message: '正在读取运行状态。',
    items: emptyDashboardItems(),
  });
  const [isRefreshingDashboard, setIsRefreshingDashboard] = useState(false);

  const refreshDashboard = useCallback(async () => {
    setIsRefreshingDashboard(true);
    try {
      const response = await loadDashboardStatus();
      if (response.operation_status === 'completed') {
        setDashboard(mapDashboardStatus(response.safe_details));
      } else {
        setDashboard({
          connectionState: 'error',
          message: response.next_recommended_action ?? response.message,
          items: emptyDashboardItems('未连接'),
        });
      }
    } catch (error) {
      setDashboard({
        connectionState: 'error',
        message: error instanceof Error ? error.message : '无法连接运行时控制后端。',
        items: emptyDashboardItems('未连接'),
      });
    } finally {
      setIsRefreshingDashboard(false);
    }
  }, []);

  useEffect(() => {
    void refreshDashboard();
    const refreshTimer = window.setInterval(() => {
      void refreshDashboard();
    }, 5000);
    return () => window.clearInterval(refreshTimer);
  }, [refreshDashboard]);

  return (
    <div className="relative flex h-screen w-full flex-col overflow-hidden bg-[#fafbfe] font-sans selection:bg-blue-200">
      {/* Subtle Generative Fluid Art Background (Light mode aesthetic) */}
      <div className="pointer-events-none absolute inset-0 z-0 opacity-80 mix-blend-multiply">
        <div className="absolute -left-[10%] -top-[10%] h-[50vh] w-[50vw] rounded-full bg-[#e8f1f9] blur-[100px]" />
        <div className="absolute -right-[5%] top-[20%] h-[60vh] w-[40vw] rounded-full bg-[#f0f5fb] blur-[120px]" />
        <div className="absolute -bottom-[10%] left-[20%] h-[40vh] w-[50vw] rounded-full bg-[#e0eff8] blur-[110px]" />
      </div>

      <TitleBar />
      
      <div className="relative z-10 flex min-h-0 flex-1 flex-col">
        <NavPill activeTab={activeTab} onTabChange={setActiveTab} />
        {activeTab === 'home' && (
          <ActionArea
            dashboard={dashboard}
            isRefreshingDashboard={isRefreshingDashboard}
            onRefreshDashboard={refreshDashboard}
          />
        )}
        <Suspense fallback={null}>
          {activeTab === 'account' && <AccountArea />}
          {activeTab === 'settings' && <SettingsArea />}
        </Suspense>
      </div>


    </div>
  );
}

function emptyDashboardItems(value = '读取中'): DashboardViewState['items'] {
  return [
    {
      id: 'service',
      title: '后台服务',
      value,
      detail: 'Service Control Manager',
      tone: 'neutral',
    },
    {
      id: 'provider',
      title: '登录组件',
      value,
      detail: 'Credential Provider',
      tone: 'neutral',
    },
    {
      id: 'config',
      title: '认证配置',
      value,
      detail: 'Service registry',
      tone: 'neutral',
    },
    {
      id: 'data',
      title: '数据目录',
      value,
      detail: 'ProgramData',
      tone: 'neutral',
    },
  ];
}

function mapDashboardStatus(status: DashboardStatus): DashboardViewState {
  return {
    connectionState: 'connected',
    message: status.presence_runtime?.reason ?? '运行状态已更新。',
    items: [
      {
        id: 'service',
        title: '后台服务',
        value: serviceValue(status),
        detail: serviceDetail(status),
        tone: status.service.runtime_state === 'running' ? 'good' : 'warn',
      },
      {
        id: 'provider',
        title: '登录组件',
        value: providerValue(status),
        detail: `CP ${boolText(status.provider.credential_provider_registered)} / COM ${boolText(
          status.provider.com_server_registered,
        )}`,
        tone: status.provider.registration_state === 'registered' ? 'good' : 'warn',
      },
      {
        id: 'config',
        title: '认证配置',
        value: status.service_config.registry_config_state === 'present' ? '已写入' : '未配置',
        detail: status.service_config.auth_mode ?? '无认证模式',
        tone: status.service_config.registry_config_state === 'present' ? 'good' : 'warn',
      },
      {
        id: 'data',
        title: '数据目录',
        value: status.data_directory.program_data_presence === 'present' ? '可用' : '缺失',
        detail: status.data_directory.program_data_dir ?? '未返回路径',
        tone: status.data_directory.program_data_presence === 'present' ? 'good' : 'warn',
      },
    ],
  };
}

function serviceValue(status: DashboardStatus) {
  if (status.service.installation_state === 'missing') {
    return '未安装';
  }
  return status.service.runtime_state === 'running' ? '运行中' : runtimeStateText(status.service.runtime_state);
}

function serviceDetail(status: DashboardStatus) {
  if (status.service.process_id) {
    return `PID ${status.service.process_id}`;
  }
  return runtimeStateText(status.service.runtime_state);
}

function providerValue(status: DashboardStatus) {
  switch (status.provider.registration_state) {
    case 'registered':
      return '已注册';
    case 'partially_registered':
      return '部分注册';
    default:
      return '未注册';
  }
}

function runtimeStateText(state: string) {
  switch (state) {
    case 'running':
      return '运行中';
    case 'stopped':
      return '已停止';
    case 'paused':
      return '已暂停';
    case 'start_pending':
      return '启动中';
    case 'stop_pending':
      return '停止中';
    case 'missing':
      return '未安装';
    default:
      return state;
  }
}

function boolText(value: boolean) {
  return value ? '是' : '否';
}
