; LiveDub Voice installer hooks, inserted into Tauri's NSIS template
; (bundle > windows > nsis > installerHooks in tauri.conf.json).

; "Delete the application data" in the uninstaller (off by default) makes Tauri's template remove
; %APPDATA%\${BUNDLEID} (settings, usage history) and %LOCALAPPDATA%\${BUNDLEID} (logs). The
; OpenAI API key is not in those folders: the app keeps it in Windows Credential Manager under
; the keyring target "<user>.<service>" (src-tauri/src/credentials), so remove it here too.
; Updates (/UPDATE) never delete anything.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:openai-api-key.${BUNDLEID}'
    Pop $0
  ${EndIf}
!macroend
