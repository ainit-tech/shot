; The installer of ainitShot (decision 13): for the current user, without administrator rights,
; into %LOCALAPPDATA%\Programs\ainitShot, with a shortcut in the Start menu, starting with Windows
; and an entry in Settings > Apps. Run silently with /S /UPDATE by the program itself, it waits
; for the program to close, replaces it and starts the new version.
;
; Built on Linux as well as on Windows:
;   makensis -INPUTCHARSET UTF8 -DVERSION=0.2.0 -DEXE=<ainitshot.exe> -DOUT=<setup.exe> installer/ainitShot.nsi

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!ifndef VERSION | EXE | OUT
  !error "VERSION, EXE and OUT are needed: makensis -DVERSION=… -DEXE=… -DOUT=…"
!endif

!define UNINSTALL "Software\Microsoft\Windows\CurrentVersion\Uninstall\ainitShot"
!define RUN "Software\Microsoft\Windows\CurrentVersion\Run"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"

Name "ainitShot"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\ainitShot"
BrandingText "ainitShot ${VERSION}"
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=0 "ProductName" "ainitShot"
VIAddVersionKey /LANG=0 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileDescription" "ainitShot ${VERSION}"
VIAddVersionKey /LANG=0 "CompanyName" "ainit"
VIAddVersionKey /LANG=0 "LegalCopyright" "GPL-3.0-or-later"

!define MUI_ICON "ainitShot.ico"
!define MUI_UNICON "ainitShot.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\ainitShot.exe"

!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; The installer speaks the language of Windows where NSIS has it, English otherwise.
!include "languages.nsh"

; Whether the program itself runs the installer to update.
Var Updating

Function .onInit
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $1
  ${IfNot} ${Errors}
    StrCpy $Updating 1
  ${EndIf}
FunctionEnd

; Closes a running ainitShot as "Exit" does, letting its browser window go properly, and waits
; until the process has ended, which releases its mutex.
!macro CLOSE_RUNNING prefix
Function ${prefix}CloseRunning
  FindWindow $0 "ainitShot.main"
  ${If} $0 <> 0
    SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=5000
  ${EndIf}
  StrCpy $1 0
  ${Do}
    System::Call 'kernel32::OpenMutexW(i 0x00100000, i 0, w "Local\ainitShot") p .r0'
    ${If} $0 == 0
      ${Break}
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p r0)'
    Sleep 250
    IntOp $1 $1 + 1
  ${LoopWhile} $1 < 120
FunctionEnd
!macroend
!insertmacro CLOSE_RUNNING ""
!insertmacro CLOSE_RUNNING "un."

Section
  Call CloseRunning
  SetOutPath "$INSTDIR"
  File "/oname=ainitShot.exe" "${EXE}"
  File "ainitShot.ico"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\ainitShot.lnk" "$INSTDIR\ainitShot.exe" "" "$INSTDIR\ainitShot.ico"

  WriteRegStr HKCU "${UNINSTALL}" "DisplayName" "ainitShot"
  WriteRegStr HKCU "${UNINSTALL}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL}" "Publisher" "ainit"
  WriteRegStr HKCU "${UNINSTALL}" "DisplayIcon" "$INSTDIR\ainitShot.ico"
  WriteRegStr HKCU "${UNINSTALL}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL}" "URLInfoAbout" "https://shot.ainit.tech"
  WriteRegStr HKCU "${UNINSTALL}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL}" "EstimatedSize" $0

  ${If} $Updating == 1
    ; An update keeps the settings, starting with Windows among them, and starts the new version.
    Exec '"$INSTDIR\ainitShot.exe" --updated'
  ${Else}
    ; The program starts with Windows from now on; its settings window turns that off. Its first
    ; settings keep what is set here.
    WriteRegStr HKCU "${RUN}" "ainitShot" '"$INSTDIR\ainitShot.exe"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  Call un.CloseRunning
  Delete "$INSTDIR\ainitShot.exe"
  Delete "$INSTDIR\ainitShot.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\ainitShot.lnk"
  DeleteRegValue HKCU "${RUN}" "ainitShot"
  DeleteRegKey HKCU "${UNINSTALL}"
  ${IfNot} ${Silent}
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "$(RemoveData)" IDNO keep
    RMDir /r "$APPDATA\ainitShot"
    RMDir /r "$LOCALAPPDATA\ainitShot"
    keep:
  ${EndIf}
SectionEnd
