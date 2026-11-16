; SPDX-License-Identifier: MIT
; Copyright (c) 2026 angiraulene1-create
; Instalador NSIS de Tlaxtrelom para Windows de 64 bits.
;
; Correr desde la raiz del arbol empaquetado
; (dist\tlaxtrelom-<ver>-windows-x86_64), que es donde estan tlaxtrelom.exe,
; assets\ y shaders\:
;     makensis tlaxtrelom.nsi
; "cargo xtask package --platform windows" hace esto automaticamente si makensis
; esta en el PATH; si no, deja este guion dentro del portable.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!define PRODUCT "Tlaxtrelom"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tlaxtrelom"

Name "Tlaxtrelom ${VERSION}"
OutFile "tlaxtrelom-setup-${VERSION}.exe"
InstallDir "$PROGRAMFILES64\Tlaxtrelom"
InstallDirRegKey HKLM "${UNINSTALL_KEY}" "InstallDir"
UninstallButton true
RequestExecutionLevel admin
SetCompressor /SOLID lzma
SetCompressorDictSize 64

!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Spanish"
!insertmacro MUI_LANGUAGE "English"

Section "Motor" SecEngine
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "tlaxtrelom.exe"
  File "tlaxtrelom.cmd"
  File "LICENSE"
  File "README.md"
  File "CHANGELOG.md"
  SetOutPath "$INSTDIR\assets"
  File /r "assets\*.*"
  SetOutPath "$INSTDIR\shaders"
  File /r "shaders\*.wgsl"

  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "Tlaxtrelom"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "angiraulene1-create"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "URLInfoAbout" \
    "https://github.com/angiraulene1-create/tlaxtrelom-engine"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Accesos directos" SecLinks
  CreateDirectory "$SMPROGRAMS\Tlaxtrelom"
  CreateShortCut "$SMPROGRAMS\Tlaxtrelom\Tlaxtrelom.lnk" \
    "$INSTDIR\tlaxtrelom.cmd" "" "$INSTDIR\tlaxtrelom.exe"
  CreateShortCut "$SMPROGRAMS\Tlaxtrelom\Documentacion.lnk" "$INSTDIR\README.md"
  CreateShortCut "$DESKTOP\Tlaxtrelom.lnk" \
    "$INSTDIR\tlaxtrelom.cmd" "" "$INSTDIR\tlaxtrelom.exe"
SectionEnd

Section "Asociar clasters .tlxg" SecAssoc
  WriteRegStr HKCR "Tlaxtrelom.Cluster" "" "Claster geometrico de Tlaxtrelom"
  WriteRegStr HKCR "Tlaxtrelom.Cluster\shell\open\command" "" \
    '"$INSTDIR\tlaxtrelom.exe" --open "%1"'
  WriteRegStr HKCR ".tlxg" "" "Tlaxtrelom.Cluster"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecEngine} "Motor, shaders y claster de demostracion."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecLinks} "Accesos directos de Menu Inicio y escritorio."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAssoc} "Abrir archivos .tlxg con doble clic."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  RMDir /r "$INSTDIR\assets"
  RMDir /r "$INSTDIR\shaders"
  Delete "$INSTDIR\tlaxtrelom.exe"
  Delete "$INSTDIR\tlaxtrelom.cmd"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Tlaxtrelom\Tlaxtrelom.lnk"
  Delete "$SMPROGRAMS\Tlaxtrelom\Documentacion.lnk"
  RMDir "$SMPROGRAMS\Tlaxtrelom"
  Delete "$DESKTOP\Tlaxtrelom.lnk"
  DeleteRegKey HKLM "${UNINSTALL_KEY}"
  DeleteRegKey HKCR "Tlaxtrelom.Cluster"
  DeleteRegValue HKCR ".tlxg" ""
SectionEnd
