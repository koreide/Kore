; NSIS installer for kore.exe (per-user, no admin prompt).
;
;   makensis /DVERSION=0.2.0 /DBIN=C:\abs\kore.exe /DOUT=C:\abs\kore_0.2.0_x64-setup.exe crates\kore-app\packaging\windows\kore.nsi
;
; makensis runs from this directory, so BIN and OUT must be absolute paths.
;
; Installs to %LOCALAPPDATA%\Programs\kore, adds Start Menu and Desktop shortcuts and an
; uninstaller registered under HKCU (Settings > Apps).
Unicode true
SetCompressor /SOLID lzma

!ifndef VERSION
  !error "pass /DVERSION=<x.y.z>"
!endif
!ifndef BIN
  !error "pass /DBIN=<path to kore.exe>"
!endif
!ifndef OUT
  !define OUT "kore_${VERSION}_x64-setup.exe"
!endif

!define PRODUCT "kore"
!define IDENTIFIER "dev.kore.desktop"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${IDENTIFIER}"
!define ICON "../../assets/icons/icon.ico"  ; makensis runs from this directory

!include "MUI2.nsh"

Name "${PRODUCT}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
InstallDirRegKey HKCU "Software\${IDENTIFIER}" "InstallDir"
RequestExecutionLevel user
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 Elad Bash (MIT)"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\kore.exe"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  ; An upgrade over a running instance would fail to overwrite kore.exe.
  nsExec::Exec 'taskkill /IM kore.exe /F'
  File "/oname=kore.exe" "${BIN}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\kore.exe"
  CreateShortcut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\kore.exe"

  WriteRegStr HKCU "Software\${IDENTIFIER}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\kore.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PRODUCT}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /IM kore.exe /F'
  Delete "$INSTDIR\kore.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\${IDENTIFIER}"
SectionEnd
