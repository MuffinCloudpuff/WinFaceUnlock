# 由 claude Ai 生成（居然能用，看来离一键卸载脚本不远了）

!macro NSIS_HOOK_PREINSTALL
  ; 检查并结束 FaceWinUnlock-Server.exe 进程
  DetailPrint "正在检查 FaceWinUnlock-Server.exe 进程..."
  
  ; 使用 taskkill 命令强制结束进程
  nsExec::ExecToStack 'taskkill /F /IM "FaceWinUnlock-Server.exe"'
  Pop $0 ; 返回码
  Pop $1 ; 输出信息
  
  ; 检查返回码
  ${If} $0 == 0
    DetailPrint "FaceWinUnlock-Server.exe 进程已成功结束"
  ${ElseIf} $0 == 128
    DetailPrint "未找到 FaceWinUnlock-Server.exe 进程（首次安装）"
  ${Else}
    DetailPrint "结束进程返回码: $0"
  ${EndIf}
  
  ; 等待一下确保进程完全结束
  Sleep 1000
  
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; 安装后的操作（如果需要）
  DetailPrint "安装完成"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; 卸载前结束进程
  DetailPrint "正在检查 FaceWinUnlock-Server.exe 进程..."
  nsExec::ExecToStack 'taskkill /F /IM "FaceWinUnlock-Server.exe"'
  Pop $0
  Pop $1
  Sleep 1000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; 卸载完成后的操作（如果需要）
  DetailPrint "卸载完成"
!macroend