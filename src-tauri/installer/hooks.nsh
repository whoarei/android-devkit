; The updater starts NSIS before exiting the old process. Let it release the
; installed executable before Tauri's Restart Manager shutdown check runs.
; See https://github.com/tauri-apps/tauri/issues/16277.
!macro NSIS_HOOK_PREINSTALL
  ${If} $UpdateMode = 1
    Push $0
    Push $1
    Push $2
    Push $3
    Push $4
    Push $5
    Push $R0

    System::Call 'kernel32::GetTickCount() i .r4'
    ${Do}
      ; Refresh resource registration for each probe, including after exit.
      !insertmacro RestartManager_StartSession $R0
      ${If} $R0 == ""
        ${ExitDo}
      ${EndIf}
      !insertmacro RestartManager_RegisterFile $R0 "$INSTDIR\${MAINBINARYNAME}.exe"
      ${If} $0 = 0
        ; Poll the exact installed path, never other copies with the same name.
        ; Reset the buffer counts on every call: no process buffer is supplied.
        StrCpy $1 0
        StrCpy $2 0
        StrCpy $3 0
        System::Call 'RSTRTMGR::RmGetList(i R0, *i .r1, *i .r2, p 0, *i .r3) i .r0'
      ${EndIf}
      ; Use an explicit output register to preserve the probe result in $0.
      System::Call 'RSTRTMGR::RmEndSession(i R0) i .r3'
      ${If} $0 != ${ERROR_MORE_DATA}
        ${ExitDo}
      ${EndIf}
      ; Include probe overhead in the deadline. Subtraction handles tick wrap.
      System::Call 'kernel32::GetTickCount() i .r5'
      IntOp $5 $5 - $4
      ${If} $5 >= 20000
        ${ExitDo}
      ${EndIf}
      Sleep 250
    ${Loop}

    Pop $R0
    Pop $5
    Pop $4
    Pop $3
    Pop $2
    Pop $1
    Pop $0
    ; On timeout or an API error, retain Tauri's normal running-app check.
  ${EndIf}
!macroend
