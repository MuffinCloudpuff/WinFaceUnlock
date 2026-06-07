# Immersive Glassmorphism Design System

## 1. 设计目标

本设计系统用于构建沉浸式、玻璃拟态、具备物理触感的高端数字界面。它强调克制、呼吸感、有机边界和动态上下文色彩，使界面看起来像漂浮在数字流体上的磨砂玻璃实体，而不是传统平面 UI。

该系统不限定为暗色主题。暗色、浅色、彩色背景、品牌渐变、专辑封面、空间场景和动态影像都可以作为底层环境；玻璃材质通过主题令牌适配不同背景的亮度、对比度和情绪。

适用场景包括：

- 流媒体播放器、歌词视图、3D 封面界面
- 高端创意工具
- 空间计算界面
- 数字展示大屏
- 需要强沉浸感和高质感反馈的产品体验

核心关键词：

- 沉浸感
- 上下文玻璃拟态
- 有机生长
- 物理触感
- 克制与呼吸感

## 2. 设计原则

1. 界面层级应通过透明度、模糊、边缘高光、阴影和空间关系建立，而不是依赖生硬分割线。
2. 所有主要容器都应具有玻璃质感，并允许底层动态背景在前景中隐约渗透。
3. 交互反馈应模拟实体材质的受力、按压、悬浮和回弹。
4. 色彩应保持克制，主色优先来自上下文动态提取，而不是固定品牌大面积铺色。
5. 信息层级应通过字体大小、字重、透明度和留白建立。
6. 动画曲线应偏向弹簧和阻尼，不使用生硬的 linear 动画。
7. 所有跨组件规范应沉淀为设计令牌，方便主题切换、白标配置和工程复用。

## 3. 色彩系统

色彩系统分为抽象令牌与主题实现。设计系统本身只规定层级、透明度关系、语义角色和交互状态；具体颜色由主题、品牌、背景图像或运行时上下文注入。

### 3.1 基础令牌

```css
:root {
  --color-bg-base: var(--theme-bg-base, #030303);
  --color-surface-base: var(--theme-surface-base, rgba(255, 255, 255, 0.03));
  --color-surface-elevated: var(--theme-surface-elevated, rgba(255, 255, 255, 0.08));
  --color-primary: var(--context-color, var(--theme-primary, #ffffff));
  --color-border-specular: var(--theme-border-specular, rgba(255, 255, 255, 0.12));
  --color-shadow-ambient: var(--theme-shadow-ambient, rgba(0, 0, 0, 0.4));
}
```

说明：

- `--color-bg-base` 是底层环境画布，可以是暗色、浅色、品牌色或动态视觉。
- `--color-surface-base` 用于基础玻璃容器。
- `--color-surface-elevated` 用于悬浮态或交互态容器。
- `--color-primary` 默认由上下文动态注入，例如环境色、封面色、当前主题色。
- `--color-border-specular` 用于模拟玻璃边缘折射高光。
- `--color-shadow-ambient` 用于适配不同主题下的环境阴影，不固定为黑色阴影。

### 3.2 主题示例

暗色主题示例：

```css
:root,
.theme-dark {
  --theme-bg-base: #030303;
  --theme-surface-base: rgba(255, 255, 255, 0.03);
  --theme-surface-elevated: rgba(255, 255, 255, 0.08);
  --theme-border-specular: rgba(255, 255, 255, 0.12);
  --theme-shadow-ambient: rgba(0, 0, 0, 0.4);
  --theme-primary: #ffffff;

  --theme-text-primary: rgba(255, 255, 255, 0.92);
  --theme-text-secondary: rgba(255, 255, 255, 0.6);
  --theme-text-disabled: rgba(255, 255, 255, 0.3);
}
```

浅色主题示例：

```css
.theme-light {
  --theme-bg-base: #f6f4ef;
  --theme-surface-base: rgba(255, 255, 255, 0.48);
  --theme-surface-elevated: rgba(255, 255, 255, 0.72);
  --theme-border-specular: rgba(255, 255, 255, 0.82);
  --theme-shadow-ambient: rgba(36, 30, 20, 0.16);
  --theme-primary: #111827;

  --theme-text-primary: rgba(17, 24, 39, 0.92);
  --theme-text-secondary: rgba(17, 24, 39, 0.64);
  --theme-text-disabled: rgba(17, 24, 39, 0.34);
}
```

### 3.3 语义色

```css
:root {
  --color-success: #34d399;
  --color-warning: #fbbf24;
  --color-error: #f87171;
  --color-info: #60a5fa;
}
```

语义色需要根据当前主题调整明度、透明度和发光强度，避免在任何背景下出现刺眼或对比不足的问题。推荐配合透明度、发光、细边框和小面积状态标识。

### 3.4 文本色

```css
:root {
  --text-primary: var(--theme-text-primary, rgba(255, 255, 255, 0.92));
  --text-secondary: var(--theme-text-secondary, rgba(255, 255, 255, 0.6));
  --text-disabled: var(--theme-text-disabled, rgba(255, 255, 255, 0.3));
}
```

文本不应直接写死为纯白或纯黑。主标题和核心数据使用 `--text-primary`，正文和说明使用 `--text-secondary`，禁用状态和弱提示使用 `--text-disabled`。具体颜色由主题决定。

## 4. 几何与排版

### 4.1 圆角

```css
:root {
  --radius-sm: 8px;
  --radius-md: 16px;
  --radius-lg: 32px;
  --radius-full: 9999px;
}
```

使用规则：

- `--radius-sm`：小标签、微型按钮、图标容器
- `--radius-md`：输入框、普通按钮、内部子卡片
- `--radius-lg`：核心功能块、模态框、播放面板
- `--radius-full`：胶囊按钮、头像、圆形操作按钮

### 4.2 字体

```css
:root {
  --font-ui: "Inter", system-ui, sans-serif;
  --font-display: "Space Grotesk", "Inter", system-ui, sans-serif;
}
```

排版层级：

```css
:root {
  --text-h1-size: 48px;
  --text-h1-line: 1.1;
  --text-h1-weight: 600;

  --text-h2-size: 32px;
  --text-h2-line: 1.2;
  --text-h2-weight: 500;

  --text-body-size: 16px;
  --text-body-line: 1.6;
  --text-body-weight: 400;

  --text-caption-size: 13px;
  --text-caption-line: 1.4;
  --text-caption-weight: 400;
}
```

主体 UI 使用 Inter 保证可读性；展示标题、核心数据和沉浸式页面标题可使用 Space Grotesk 增强科技感。

### 4.3 间距

```css
:root {
  --spacing-xs: 4px;
  --spacing-sm: 8px;
  --spacing-md: 16px;
  --spacing-lg: 32px;
}
```

所有布局应基于 8px 网格。核心卡片和主要模块优先使用 `--spacing-lg` 作为内部留白，保持呼吸感。

## 5. 光影与层级

### 5.1 玻璃层级

```css
:root {
  --glass-base-bg: linear-gradient(
    135deg,
    rgba(255, 255, 255, 0.05) 0%,
    rgba(255, 255, 255, 0.01) 100%
  );
  --glass-heavy-bg: rgba(10, 10, 10, 0.65);

  --glass-blur-base: blur(40px) saturate(150%);
  --glass-blur-elevated: blur(60px) saturate(180%);
  --glass-blur-modal: blur(100px) saturate(200%);

  --shadow-elevation-1: 0 8px 32px rgba(0, 0, 0, 0.4);
  --shadow-elevation-2: 0 16px 48px rgba(0, 0, 0, 0.6);
}
```

层级规则：

- Elevation 0：底层动态环境，通常是流体背景、极光、粒子或 WebGL 场景。
- Elevation 1：基础玻璃卡片，使用 40px 以上 backdrop blur。
- Elevation 2：下拉、浮层、popover，使用更强模糊和阴影。
- Elevation 3：全局模态框，使用极高模糊并配合全屏视距聚焦遮罩。

### 5.2 Z-Index

```css
:root {
  --z-background: 0;
  --z-content: 5;
  --z-shell: 10;
  --z-popover: 40;
  --z-modal: 50;
}
```

布局层级：

- `z-0`：动态背景，占满 `100vw` / `100vh`。
- `z-5`：主内容区，承载可滚动内容。
- `z-10`：侧边导航、底部控制栏等系统主干。
- `z-40`：菜单、浮层、Toast。
- `z-50`：模态框和顶层确认流程。

## 6. 动效与交互

### 6.1 动画曲线

```css
:root {
  --ease-spring-gentle: cubic-bezier(0.25, 1, 0.5, 1);
  --ease-spring-bouncy: cubic-bezier(0.175, 0.885, 0.32, 1.275);
  --ease-fluid-color: cubic-bezier(0.4, 0, 0.2, 1);
}
```

使用规则：

- `--ease-spring-gentle`：弹窗、卡片、尺寸变化。
- `--ease-spring-bouncy`：按钮释放、轻度回弹。
- `--ease-fluid-color`：背景颜色、渐变和上下文色切换。

### 6.2 时长

```css
:root {
  --duration-fast: 150ms;
  --duration-base: 300ms;
  --duration-slow: 800ms;
}
```

使用规则：

- `150ms`：hover、active、即时响应。
- `300ms`：卡片悬浮、抽屉展开、普通状态切换。
- `600ms` 到 `1000ms`：背景流体变化、大面积页面转场。

### 6.3 状态反馈

Hover：

- 背景亮度提升 5% 到 10%。
- 可增加轻微外发光。
- 卡片可向上浮动 `translateY(-2px)`。

Active：

- 使用缩放模拟按压。
- 按钮建议使用 `transform: scale(0.92)`。
- 普通交互元素可使用 `transform: scale(0.96) translateZ(0)`。

## 7. 核心组件

### 7.1 沉浸式玻璃信息卡片

用途：承载歌单、动态、大图面板、主内容模块等信息。

```css
.glass-card {
  border-radius: var(--radius-lg);
  padding: var(--spacing-lg);
  color: var(--text-primary);
  background: var(--glass-base-bg);
  border: 1px solid var(--color-border-specular);
  box-shadow: var(--shadow-elevation-1);
  backdrop-filter: blur(80px) saturate(150%);
  transition:
    transform var(--duration-base) var(--ease-spring-gentle),
    box-shadow var(--duration-base) var(--ease-spring-gentle),
    background var(--duration-base) var(--ease-spring-gentle);
  will-change: transform, filter;
}

.glass-card:hover {
  transform: translateY(-2px);
  box-shadow:
    var(--shadow-elevation-2),
    0 0 16px rgba(255, 255, 255, 0.05);
}
```

### 7.2 光晕核心感应按钮

用途：播放、连接、立即体验等关键操作。

```css
.tactile-button {
  height: 48px;
  border-radius: var(--radius-full);
  padding: 0 24px;
  color: var(--text-primary);
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.15);
  backdrop-filter: blur(24px) saturate(150%);
  transition:
    transform var(--duration-fast) var(--ease-spring-gentle),
    background var(--duration-fast) var(--ease-spring-gentle),
    box-shadow var(--duration-fast) var(--ease-spring-gentle);
  will-change: transform;
}

.tactile-button:hover {
  background: rgba(255, 255, 255, 0.2);
  box-shadow:
    inset 0 0 12px rgba(255, 255, 255, 0.08),
    0 0 20px color-mix(in srgb, var(--color-primary) 30%, transparent);
}

.tactile-button:active {
  transform: scale(0.92);
}
```

## 8. 表单与输入控件

### 8.1 输入框

```css
:root {
  --surface-input: rgba(255, 255, 255, 0.03);
  --border-input: rgba(255, 255, 255, 0.08);
  --shadow-inset-soft: inset 0 2px 4px rgba(0, 0, 0, 0.2);
  --ring-focus: 0 0 0 1px var(--color-primary),
    0 0 12px color-mix(in srgb, var(--color-primary) 30%, transparent);
  --opacity-disabled: 0.3;
}

.glass-input {
  border-radius: var(--radius-md);
  color: var(--text-primary);
  background: var(--surface-input);
  border: 1px solid var(--border-input);
  box-shadow: var(--shadow-inset-soft);
  outline: none;
  transition:
    background var(--duration-fast) var(--ease-spring-gentle),
    border-color var(--duration-fast) var(--ease-spring-gentle),
    box-shadow var(--duration-fast) var(--ease-spring-gentle);
}

.glass-input:hover {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.15);
}

.glass-input:focus {
  background: rgba(255, 255, 255, 0.08);
  box-shadow: var(--shadow-inset-soft), var(--ring-focus);
}

.glass-input:disabled {
  opacity: var(--opacity-disabled);
  cursor: not-allowed;
  pointer-events: none;
}
```

### 8.2 切换开关

设计规则：

- 关闭轨道使用深色内凹玻璃槽。
- 开启轨道使用主题色流光注入。
- 滑块使用高亮玻璃珠或银白金属颗粒质感。
- 动效使用弹簧曲线，避免机械位移感。

```css
.glass-switch {
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.08);
  backdrop-filter: blur(12px);
}

.glass-switch[data-state="checked"] {
  background: color-mix(in srgb, var(--color-primary) 80%, transparent);
  box-shadow: 0 0 16px color-mix(in srgb, var(--color-primary) 30%, transparent);
}

.glass-switch-thumb {
  background: rgba(255, 255, 255, 0.92);
  box-shadow:
    0 2px 4px rgba(0, 0, 0, 0.4),
    inset 0 1px 1px rgba(255, 255, 255, 0.8);
  transition: transform var(--duration-base) var(--ease-spring-bouncy);
}
```

## 9. 响应式布局

### 9.1 断点

```css
:root {
  --bp-sm: 640px;
  --bp-md: 768px;
  --bp-lg: 1024px;
  --bp-xl: 1280px;
}
```

响应式策略：

- `640px`：手机横屏和小型平板，侧边栏可折叠为抽屉。
- `768px`：平板纵向，多列布局可降级为单列或瀑布流。
- `1024px`：桌面起始，允许完整三栏架构。
- `1280px`：超宽屏，可启用最大内容宽度和弹性留白。

### 9.2 布局骨架

推荐结构：

```text
App Root
├── Ambient Background        z-0
├── Main Content              z-5
├── Sidebar / Bottom Bar      z-10
├── Popover / Toast           z-40
└── Modal                     z-50
```

主干面板建议使用：

```css
.glass-shell {
  background: var(--glass-heavy-bg);
  border-color: rgba(255, 255, 255, 0.06);
  backdrop-filter: blur(80px) saturate(150%);
}
```

## 10. 反馈与异步状态

### 10.1 Toast

Toast 应是轻薄的半悬浮玻璃，边框根据语义色和当前主题变化。

```css
.glass-toast {
  background: rgba(20, 20, 20, 0.7);
  border: 1px solid color-mix(in srgb, var(--toast-color) 50%, transparent);
  border-radius: var(--radius-md);
  backdrop-filter: blur(24px) saturate(150%);
  animation: toast-enter 400ms var(--ease-spring-bouncy);
}

.glass-toast[data-exit="true"] {
  animation: toast-fade-out 200ms ease-in forwards;
}

@keyframes toast-enter {
  from {
    opacity: 0;
    transform: translateY(12px) scale(0.96);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

@keyframes toast-fade-out {
  to {
    opacity: 0;
    filter: blur(8px);
    transform: scale(0.95);
  }
}
```

### 10.2 骨架屏

骨架屏应表现为流光扫过低对比玻璃，而不是生硬的灰色占位块。

```css
.glass-skeleton {
  position: relative;
  overflow: hidden;
  border-radius: var(--radius-md);
  background: rgba(255, 255, 255, 0.02);
}

.glass-skeleton::after {
  content: "";
  position: absolute;
  inset: 0;
  transform: translateX(-100%);
  background: linear-gradient(
    110deg,
    transparent 0%,
    rgba(255, 255, 255, 0.16) 45%,
    transparent 70%
  );
  mix-blend-mode: overlay;
  animation: skeleton-shimmer 1400ms infinite;
}

@keyframes skeleton-shimmer {
  to {
    transform: translateX(100%);
  }
}
```

全局加载建议使用背景极光的缓慢呼吸变化，例如 opacity、blur 或 hue-rotate，而不是传统圆形 Spinner。

## 11. 图标与滚动条

### 11.1 图标

图标风格：

- 使用线性几何图标。
- 避免大面积实心图标。
- 默认笔划宽度为 `1.5px`。
- 端点和连接点使用圆角。
- 默认颜色使用 `--text-secondary`，hover 时提升到 `--text-primary`。

```css
.glass-icon {
  color: var(--text-secondary);
  stroke-width: 1.5px;
  stroke-linecap: round;
  stroke-linejoin: round;
  transition:
    color var(--duration-fast) var(--ease-spring-gentle),
    filter var(--duration-fast) var(--ease-spring-gentle);
}

.glass-icon:hover {
  color: var(--text-primary);
  filter: drop-shadow(0 0 4px rgba(255, 255, 255, 0.4));
}
```

### 11.2 滚动条

```css
* {
  scrollbar-width: thin;
  scrollbar-color: rgba(255, 255, 255, 0.12) transparent;
}

*::-webkit-scrollbar {
  width: 12px;
  height: 12px;
}

*::-webkit-scrollbar-track {
  background: transparent;
}

*::-webkit-scrollbar-thumb {
  border: 4px solid transparent;
  border-radius: var(--radius-full);
  background: rgba(255, 255, 255, 0.12);
  background-clip: padding-box;
}

*::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.3);
  background-clip: padding-box;
}

*::-webkit-scrollbar-button {
  display: none;
}
```

## 12. 工程落地建议

### 12.1 Token 管理

设计令牌应集中维护在全局主题文件中，例如：

- `theme.css`
- `tokens.css`
- `tailwind.config.js`
- CSS-in-JS theme object

推荐将以下内容全部令牌化：

- 色彩
- 文本透明度
- 圆角
- 间距
- 阴影
- 模糊强度
- 动画曲线
- 动画时长
- Z-Index
- 语义状态色

### 12.2 性能要求

玻璃拟态依赖大量 `backdrop-filter` 和动态背景，必须关注 GPU 合成与重绘成本。

关键要求：

- 将重度模糊元素放在独立合成层。
- 对高频交互元素使用 `will-change: transform`。
- 动画优先修改 `transform`、`opacity`，避免频繁触发布局和重绘。
- 不在滚动列表中无限制堆叠高强度 `backdrop-filter`。
- 大面积流体背景应优先使用 WebGL、WebGPU、Canvas 或低频 CSS 动画。
- 背景颜色切换使用较慢曲线，避免突兀闪烁。

### 12.3 可访问性

尽管视觉系统强调透明材质和动态环境，仍需保证基础可访问性。

要求：

- 正文与背景保持足够对比度。
- 禁用态不能只依赖颜色表达。
- 聚焦态必须可见，且不能保留浏览器默认突兀 outline。
- 动效应尊重 `prefers-reduced-motion`。
- 关键交互控件应支持键盘导航。

示例：

```css
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 1ms !important;
    animation-iteration-count: 1 !important;
    scroll-behavior: auto !important;
    transition-duration: 1ms !important;
  }
}
```

## 13. Tailwind 映射建议

如使用 Tailwind，可将核心令牌映射到扩展主题：

```js
export default {
  theme: {
    extend: {
      colors: {
        bg: {
          base: "var(--color-bg-base)",
        },
        text: {
          primary: "var(--text-primary)",
          secondary: "var(--text-secondary)",
          disabled: "var(--text-disabled)",
        },
      },
      borderRadius: {
        sm: "8px",
        md: "16px",
        lg: "32px",
        full: "9999px",
      },
      spacing: {
        xs: "4px",
        sm: "8px",
        md: "16px",
        lg: "32px",
      },
      transitionTimingFunction: {
        "spring-gentle": "cubic-bezier(0.25, 1, 0.5, 1)",
        "spring-bouncy": "cubic-bezier(0.175, 0.885, 0.32, 1.275)",
        "fluid-color": "cubic-bezier(0.4, 0, 0.2, 1)",
      },
    },
  },
};
```

## 14. 实施检查清单

- 是否使用合适的底层环境画布并保留动态背景渗透？
- 是否将主容器设计为玻璃材质，而不是实色卡片？
- 是否通过透明度和留白建立文本层级？
- 是否避免了高饱和语义色的大面积铺色？
- 是否为按钮、卡片、输入框提供 hover、active、focus 状态？
- 是否使用弹簧曲线表达物理反馈？
- 是否为 Toast、Skeleton、Loader 提供符合玻璃材质系统的异步反馈？
- 是否统一了圆角、间距、阴影、模糊和 z-index？
- 是否处理了滚动条、图标、禁用态等细节？
- 是否考虑了 `backdrop-filter` 的性能成本？
- 是否支持 `prefers-reduced-motion` 和键盘可访问性？

## 15. 总结

该设计系统的核心不是某一种固定主题或简单毛玻璃效果，而是一套以动态背景、透明材质、上下文色彩、物理反馈和工程化令牌为基础的沉浸式界面架构。它要求视觉、交互和性能同时成立：界面要有高端质感，也要具备可维护、可扩展、可主题化和可持续演进的工程基础。
