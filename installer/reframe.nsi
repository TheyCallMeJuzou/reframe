; reframe's installer (NSIS 3). Per user, no admin rights, no questions: it installs and starts reframe.
;
; It installs where the Electron version's installer (electron-builder, one-click, per user) did, under the same
; uninstall entry, so that it installs over an installed 0.x, also when that copy's updater runs it on quit
; (`--updated /S`) or for "Restart to update" (`--updated --force-run`). Then it removes the Electron files, keeps
; the user's data (%APPDATA%), the shortcuts and the entry that starts reframe with the system, and clears what the
; updater downloaded. scripts\package.ps1 builds it; scripts\test-installer.ps1 tries it out under other names.
;
; Defined by the build: VERSION (1.0.0), VERSION_NUMBERS (1.0.0.0), SOURCE (the folder with reframe.exe and what
; goes with it), ICON, OUTFILE, APP_ID and PROFILE_FOLDER (as the app has them). The tests also give PRODUCT, GUID
; and UPDATER_CACHE.

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include LogicLib.nsh
!include FileFunc.nsh

!ifndef PRODUCT
  !define PRODUCT "reframe"
!endif
; electron-builder's id for the app id dev.reframe.app: the uninstall entry and the install folder's key.
!ifndef GUID
  !define GUID "ea9a8b13-e01f-5528-b28d-ed2ced221f23"
!endif
!ifndef UPDATER_CACHE
  !define UPDATER_CACHE "reframe-updater"
!endif
!define EXE "reframe.exe"
!define UNINSTALLER "Uninstall reframe.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${GUID}"
!define INSTALL_KEY "Software\${GUID}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define STARTUP_APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
!define DESCRIPTION "Keep your Claude Code sessions side by side in one window: tile and group them, take over the ones running in other terminals, and get every session back after a restart."
!define WEBSITE "https://github.com/TheyCallMeJuzou/reframe"

Name "${PRODUCT}"
Caption "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
Icon "${ICON}"
UninstallIcon "${ICON}"
BrandingText " "
ShowInstDetails nevershow
ShowUninstDetails nevershow
AutoCloseWindow true

VIProductVersion "${VERSION_NUMBERS}"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "reframe Team"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 reframe Team"

LoadLanguageFile "${NSISDIR}\Contrib\Language files\English.nlf"
LoadLanguageFile "${NSISDIR}\Contrib\Language files\German.nlf"

; The texts of electron-builder's installer, in reframe's languages.
LangString installing ${LANG_ENGLISH} "Installing, please wait..."
LangString installing ${LANG_GERMAN} "Installation läuft, bitte warten..."
LangString appRunning ${LANG_ENGLISH} "${PRODUCT} is running.$\nClick OK to close it.$\nIf it doesn't close, try closing it manually."
LangString appRunning ${LANG_GERMAN} "${PRODUCT} wird gerade ausgeführt.$\nKlicke auf »OK«, um es zu schließen.$\nFalls es sich nicht schließen lässt, versuche es manuell zu schließen."
LangString appCannotBeClosed ${LANG_ENGLISH} "${PRODUCT} cannot be closed. $\nPlease close it manually and click Retry to continue."
LangString appCannotBeClosed ${LANG_GERMAN} "${PRODUCT} kann nicht geschlossen werden. $\nSchließ es bitte manuell und klicke auf »Wiederholen«, um fortzufahren."
LangString areYouSureToUninstall ${LANG_ENGLISH} "Are you sure you want to uninstall ${PRODUCT}?"
LangString areYouSureToUninstall ${LANG_GERMAN} "Sind Sie sicher, dass Sie ${PRODUCT} deinstallieren möchten?"

Page instfiles
UninstPage instfiles

Var updated
Var forceRun
Var keepShortcuts

; ---------- reframe running from the install folder ----------

; Whether reframe (or a console of one of its sessions) runs from $INSTDIR: a program that runs cannot be opened
; for writing. Result on the stack: 1 or 0.
!macro DEFINE_IS_RUNNING prefix
Function ${prefix}isRunning
  Push $R0
  Push $R1
  StrCpy $R1 0
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    ClearErrors
    FileOpen $R0 "$INSTDIR\${EXE}" a
    ${If} ${Errors}
      StrCpy $R1 1
    ${Else}
      FileClose $R0
    ${EndIf}
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\OpenConsole.exe"
    ClearErrors
    FileOpen $R0 "$INSTDIR\OpenConsole.exe" a
    ${If} ${Errors}
      StrCpy $R1 1
    ${Else}
      FileClose $R0
    ${EndIf}
  ${EndIf}
  Pop $R0
  Exch $R1
FunctionEnd

; Ends every program that runs from $INSTDIR (the path goes to PowerShell through the environment, so that no
; character in it needs quoting).
Function ${prefix}killRunning
  System::Call 'kernel32::SetEnvironmentVariable(t "REFRAME_INSTDIR", t "$INSTDIR")'
  nsExec::Exec `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "Get-CimInstance Win32_Process | Where-Object { $$_.ExecutablePath -and $$_.ExecutablePath.StartsWith($$env:REFRAME_INSTDIR + '\', [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { Stop-Process -Id $$_.ProcessId -Force -ErrorAction SilentlyContinue }"`
  Pop $0
FunctionEnd

; Waits for reframe to be gone. Run by its updater, it is on its way out (it ends its sessions first, which takes up
; to 15 seconds). Run by hand, the user is asked first; a native reframe is then asked to quit as the user would
; (its sessions resume on its next start), the Electron version is ended. Whatever is left after a while is ended.
Function ${prefix}waitForReframe
  Call ${prefix}isRunning
  Pop $0
  ${If} $0 == 0
    Return
  ${EndIf}
  StrCpy $R2 40
  ${If} $updated != 1
    MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "$(appRunning)" /SD IDOK IDOK +2
    Quit
    ${If} ${FileExists} "$INSTDIR\resources\app.asar"
      StrCpy $R2 0
    ${Else}
      nsExec::Exec `"$INSTDIR\${EXE}" --quit`
      Pop $0
    ${EndIf}
  ${EndIf}
  ; Half seconds to wait.
  ${DoWhile} $R2 > 0
    Sleep 500
    IntOp $R2 $R2 - 1
    Call ${prefix}isRunning
    Pop $0
    ${If} $0 == 0
      Return
    ${EndIf}
  ${Loop}
  retry:
  Call ${prefix}killRunning
  Sleep 1000
  Call ${prefix}isRunning
  Pop $0
  ${If} $0 == 1
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(appCannotBeClosed)" /SD IDCANCEL IDRETRY retry
    Quit
  ${EndIf}
FunctionEnd
!macroend

!insertmacro DEFINE_IS_RUNNING ""
!insertmacro DEFINE_IS_RUNNING "un."

; ---------- installing ----------

Function .onInit
  ${GetParameters} $R0
  StrCpy $updated 0
  StrCpy $forceRun 0
  ClearErrors
  ${GetOptions} $R0 "--updated" $R1
  ${IfNot} ${Errors}
    StrCpy $updated 1
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "--force-run" $R1
  ${IfNot} ${Errors}
    StrCpy $forceRun 1
  ${EndIf}
  ; One installer at a time.
  System::Call 'kernel32::CreateMutex(p 0, i 0, t "${PRODUCT}-${GUID}-installer") p .r1 ?e'
  Pop $R2
  ${If} $R2 == 183
    Quit
  ${EndIf}
  ; Where an earlier copy was installed, unless /D= says otherwise.
  ${If} $INSTDIR == "$LOCALAPPDATA\Programs\${PRODUCT}"
    ReadRegStr $R1 HKCU "${INSTALL_KEY}" InstallLocation
    ${If} $R1 != ""
      StrCpy $INSTDIR $R1
    ${EndIf}
  ${EndIf}
FunctionEnd

Section "Install"
  SetDetailsPrint textonly
  DetailPrint "$(installing)"
  SetDetailsPrint none
  Call waitForReframe

  ; Shortcuts the user kept, moved or deleted stay as they are when reframe is installed over itself.
  StrCpy $keepShortcuts 0
  ReadRegStr $R0 HKCU "${INSTALL_KEY}" KeepShortcuts
  ${If} $R0 == "true"
  ${AndIf} ${FileExists} "$INSTDIR\${EXE}"
    StrCpy $keepShortcuts 1
  ${EndIf}

  ; The Electron version's files (0.x), then this version's.
  RMDir /r "$INSTDIR\locales"
  RMDir /r "$INSTDIR\resources"
  Delete "$INSTDIR\*.pak"
  Delete "$INSTDIR\*.dll"
  Delete "$INSTDIR\*.bin"
  Delete "$INSTDIR\*.dat"
  Delete "$INSTDIR\LICENSE.electron.txt"
  Delete "$INSTDIR\LICENSES.chromium.html"
  Delete "$INSTDIR\vk_swiftshader_icd.json"
  Delete "$INSTDIR\uninstallerIcon.ico"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\OpenConsole.exe"
  Delete "$INSTDIR\${UNINSTALLER}"

  SetOutPath "$INSTDIR"
  File /r "${SOURCE}\*.*"
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"

  WriteRegStr HKCU "${INSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${INSTALL_KEY}" "KeepShortcuts" "true"
  WriteRegStr HKCU "${INSTALL_KEY}" "ShortcutName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT} ${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "reframe Team"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Comments" "${DESCRIPTION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "HelpLink" "${WEBSITE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "${WEBSITE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLUpdateInfo" "${WEBSITE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Readme" "${WEBSITE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}" /currentuser'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /currentuser /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $R0 $R1 $R2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $R0

  ${If} $keepShortcuts == 0
    CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0 "" "" "${DESCRIPTION}"
    nsExec::Exec `"$INSTDIR\${EXE}" --shortcut-app-id "$SMPROGRAMS\${PRODUCT}.lnk" "${APP_ID}"`
    Pop $R0
    CreateShortCut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0 "" "" "${DESCRIPTION}"
    nsExec::Exec `"$INSTDIR\${EXE}" --shortcut-app-id "$DESKTOP\${PRODUCT}.lnk" "${APP_ID}"`
    Pop $R0
    System::Call 'shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
  ${EndIf}

  ; Starting with the system stays on, for this reframe.
  ReadRegStr $R0 HKCU "${RUN_KEY}" "${APP_ID}"
  ${If} $R0 != ""
    WriteRegStr HKCU "${RUN_KEY}" "${APP_ID}" '"$INSTDIR\${EXE}" --autostart'
  ${EndIf}

  ; What updaters downloaded (also the Electron version's). The installer that runs from there stays until it is
  ; done; reframe clears the rest on its next check.
  RMDir /r "$LOCALAPPDATA\${UPDATER_CACHE}"

  ; Started by hand, or for "Restart to update": reframe starts. Installed on quit, it does not.
  ${If} $forceRun == 1
  ${OrIfNot} ${Silent}
    Exec '"$INSTDIR\${EXE}"'
  ${EndIf}
SectionEnd

; ---------- uninstalling ----------

Function un.onInit
  StrCpy $updated 0
  ${IfNot} ${Silent}
    MessageBox MB_OKCANCEL "$(areYouSureToUninstall)" IDOK +2
    Quit
  ${EndIf}
FunctionEnd

Section "Uninstall"
  Call un.waitForReframe
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\conpty.dll"
  Delete "$INSTDIR\OpenConsole.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\${UNINSTALLER}"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  System::Call 'shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
  ; It no longer starts with the system, and Windows forgets its notifications' name.
  DeleteRegValue HKCU "${RUN_KEY}" "${APP_ID}"
  DeleteRegValue HKCU "${STARTUP_APPROVED_KEY}" "${APP_ID}"
  DeleteRegKey HKCU "Software\Classes\AppUserModelId\${APP_ID}"
  RMDir /r "$LOCALAPPDATA\${UPDATER_CACHE}"

  ; The sessions, windows and settings stay, unless asked for.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "--delete-app-data" $R1
  ${IfNot} ${Errors}
    RMDir /r "$APPDATA\${PROFILE_FOLDER}"
  ${EndIf}

  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${INSTALL_KEY}"
SectionEnd
