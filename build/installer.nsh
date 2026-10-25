; Additions to electron-builder's NSIS installer (it includes build/installer.nsh by default).

; Uninstalling also takes reframe out of the system's startup ("Start with the system" in the tray menu). Electron
; keeps that in the user's Run key as a value named after the app's AppUserModelId (APP_ID, see src/main/index.ts),
; and its on/off state under StartupApproved. Not when a new version is installed over this one: that runs this
; uninstaller with --updated (isUpdated), and starting with the system stays on.
!macro customUnInstall
  ${ifNot} ${isUpdated}
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_ID}"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${APP_ID}"
  ${endIf}
!macroend
