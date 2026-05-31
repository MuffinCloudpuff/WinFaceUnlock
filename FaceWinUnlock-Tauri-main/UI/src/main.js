import { createApp, nextTick } from "vue";
import App from "./App.vue";
import * as ElementPlusIconsVue from '@element-plus/icons-vue'
import router from "./router";
import ElementPlus from 'element-plus'
import 'element-plus/dist/index.css'
import { createPinia } from 'pinia'
import { useFile } from "./hook/useFile";
import { warn } from "@tauri-apps/plugin-log";
import { formatObjectString } from "./utils/function";

const pinia = createPinia()
const app = createApp(App)
const { read } = useFile();

for (const [key, component] of Object.entries(ElementPlusIconsVue)) {
  app.component(key, component)
}

app.use(router)
app.use(ElementPlus)
app.use(pinia)
app.mount("#app");

// 面容列表图片自定义指令
const handleFaceImage = async (el, binding) => {
  const { json_data, face_token } = binding.value || {};
  if (el._blobUrl) {
    console.log('释放内存')
    URL.revokeObjectURL(el._blobUrl);
    el._blobUrl = null;
  }
  if (!json_data || !json_data.view) {
    el.removeAttribute('src');
    return;
  }
  try {
    const blob = await read(
      'faces\\' + face_token + '.faceimg',
      'blob'
    );
    const blobUrl = URL.createObjectURL(blob);
    el.src = blobUrl;
    el._blobUrl = blobUrl;
  } catch (error) {
    const info = formatObjectString("加载图片失败：", error);
    warn(info);
    el.removeAttribute('src');
  }
};

app.directive('face-img', {
  // 组件挂载时执行
  async mounted(el, binding) {
    await handleFaceImage(el, binding);
  },
  async updated(el, binding) {  
    // 延迟到下一次DOM更新，确保状态同步
    await nextTick();
    await handleFaceImage(el, binding);
  },
  // 组件卸载时释放内存
  unmounted(el) {
    if (el._blobUrl) {
      console.log('释放内存')
      URL.revokeObjectURL(el._blobUrl);
      delete el._blobUrl;
    }
  }
});