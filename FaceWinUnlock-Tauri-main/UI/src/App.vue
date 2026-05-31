<script setup>
	import { ref } from 'vue';
	import { RouterView } from 'vue-router';
	import { connect } from './utils/sqlite.js';
	import { formatObjectString } from './utils/function.js';
	import { ElMessage, ElMessageBox } from 'element-plus';
	import { useOptionsStore } from "./stores/options";
	import { useRouter, useRoute } from 'vue-router';
	import { invoke } from '@tauri-apps/api/core';
	import { info, warn } from '@tauri-apps/plugin-log';
	import { useFacesStore } from './stores/faces.js';
	import { attachConsole } from "@tauri-apps/plugin-log";
	import { resourceDir } from '@tauri-apps/api/path';
	import { getVersion } from '@tauri-apps/api/app';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

	const isInit = ref(false);
	const router = useRouter();
	const route = useRoute();
	const optionsStore = useOptionsStore();
	const facesStore = useFacesStore();
	const currentWindow = getCurrentWindow();

	// 打包时注释
	attachConsole();

	resourceDir().then((result)=>{
		localStorage.setItem('exe_dir', result);
		return connect();
	}).then(()=>{
		return optionsStore.init();
	}).then(()=>{
		return invoke("init_model");
	}).then(()=>{
		return facesStore.init();
	}).then(()=>{
		let is_initialized = optionsStore.getOptionByKey('is_initialized');
		if(is_initialized.index == -1 || is_initialized.data.val != 'true'){
			warn("程序未初始化，强制跳转初始化界面");
			router.replace('/init');
		} else {
			// 判断登录安全
			if(optionsStore.getOptionValueByKey("loginEnabled") === "true" && 
				(optionsStore.getOptionValueByKey("loginMethod") === "onlyOpenApp" || optionsStore.getOptionValueByKey("loginMethod") === "showApp")
			){
				info("登录安全已启用，跳转登录界面");
				router.replace('/login');
			}
		}
		info("程序初始化完成");
		if(optionsStore.getOptionValueByKey('silentRun') != "true"){
			currentWindow.isVisible().then((visible) => {
				if(!visible){
					currentWindow.show();
				}
				currentWindow.setFocus();
			}).catch((error)=>{
				warn(formatObjectString("获取窗口状态失败 ",error));
			})
		}
		isInit.value = true;

		const appWindow = getCurrentWebviewWindow();
		// 监听获取焦点
		appWindow.onFocusChanged(({ payload: focused }) => {
			// 当前有焦点，并且初始化完成
			if (focused && optionsStore.getOptionValueByKey("is_initialized") === "true" && localStorage.getItem("proactiveOutOfFocus") !== "true") {
				// 判断是否开启了登录安全
				if(optionsStore.getOptionValueByKey("loginEnabled") === "true" && route.path !== '/login'){
					// 判断登录页面的显示方法
					const loginMethod = optionsStore.getOptionValueByKey("loginMethod");
					let loginLog = false;
					if(loginMethod === "showApp"){
						router.replace('/login');
						loginLog = true;
					} else if(loginMethod.includes("time:")){
						let time = parseInt(loginMethod.split(":")[1]);
						const lastLoginTime = localStorage.getItem("lastLoginTime") || '0';
						const currentTime = Date.now();
						if(currentTime - parseInt(lastLoginTime) >= time * 60 * 1000){
							router.replace('/login');
							loginLog = true;
						}
					} else if(loginMethod === "onlyOpenApp"){
						// 已经在上面处理过了
					} else {
						warn("未知的登录显示方法：" + optionsStore.getOptionValueByKey("loginMethod"));
					}

					if(loginLog){
						info("需要进行登录认证");
					}
				}
			}
		});
	}).catch((error)=>{
		ElMessageBox.alert(formatObjectString(error), '程序初始化失败', {
			confirmButtonText: '确定',
			callback: (action) => {
				invoke("close_app");
			}
		});
	})

	// 版本号不影响运行，不用放在上面
	getVersion().then((v)=>{
		localStorage.setItem('version', v);
	});
</script>

<template>
	<div class="app-wrapper" v-if="isInit">
		<router-view />
    </div>
</template>

<style scoped>
	.app-wrapper {
		height: 100vh;
		width: 100vw;
	}
</style>

<style>
	.el-message-box__content{
		user-select: text;
	}
</style>