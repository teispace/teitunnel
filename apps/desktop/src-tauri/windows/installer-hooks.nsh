; Teitunnel's NSIS installer hooks (Tauri `bundle.windows.nsis.installerHooks`).
;
; The installer puts the `teitunnel` command on the PATH: a copy of the bundled
; teitunnel-cli.exe, named teitunnel.exe, in
; %LOCALAPPDATA%\Microsoft\WindowsApps, which is on every user's PATH and needs no
; administrator. It's the same copy Settings > General > Command line makes, with the same
; marker file next to it (teitunnel.teitunnel, core::cli_install), so the app, the
; installer and the uninstaller agree on what is Teitunnel's. A teitunnel.exe there
; without the marker belongs to someone else and is never replaced or removed.

!define TEITUNNEL_CLI_DIR "$LOCALAPPDATA\Microsoft\WindowsApps"
!define TEITUNNEL_CLI "${TEITUNNEL_CLI_DIR}\teitunnel.exe"
!define TEITUNNEL_CLI_MARKER "${TEITUNNEL_CLI_DIR}\teitunnel.teitunnel"

!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "${TEITUNNEL_CLI}"
  ${AndIfNot} ${FileExists} "${TEITUNNEL_CLI_MARKER}"
    DetailPrint "Leaving ${TEITUNNEL_CLI}: it isn't Teitunnel's."
  ${ElseIf} ${FileExists} "$INSTDIR\teitunnel-cli.exe"
    CreateDirectory "${TEITUNNEL_CLI_DIR}"
    ; Copy beside it, then swap, so a running teitunnel is never half written.
    CopyFiles /SILENT "$INSTDIR\teitunnel-cli.exe" "${TEITUNNEL_CLI}.partial"
    Delete "${TEITUNNEL_CLI}"
    Rename "${TEITUNNEL_CLI}.partial" "${TEITUNNEL_CLI}"
    FileOpen $0 "${TEITUNNEL_CLI_MARKER}" w
    FileWrite $0 "Installed by Teitunnel; updated when the app updates.$\n"
    FileClose $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} ${FileExists} "${TEITUNNEL_CLI_MARKER}"
    Delete "${TEITUNNEL_CLI}"
    Delete "${TEITUNNEL_CLI_MARKER}"
  ${EndIf}
!macroend
