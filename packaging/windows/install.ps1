# SPDX-License-Identifier: MIT
# Copyright (c) 2026 angiraulene1-create
# Script instalador para Windows PowerShell (Tlaxtrelom Engine)
# Crea accesos directos en el Menu Inicio y Escritorio, y asocia la extension .tlxg

[CmdletBinding()]
param (
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\Tlaxtrelom",
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"

if ($Uninstall) {
    Write-Host "==> Desinstalando Tlaxtrelom Engine..." -ForegroundColor Yellow
    $desktopLnk = "$env:USERPROFILE\Desktop\Tlaxtrelom.lnk"
    $startMenuDir = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Tlaxtrelom"
    if (Test-Path $desktopLnk) { Remove-Item -Force $desktopLnk }
    if (Test-Path $startMenuDir) { Remove-Item -Recurse -Force $startMenuDir }
    if (Test-Path $InstallDir) { Remove-Item -Recurse -Force $InstallDir }
    Write-Host "Desinstalacion completada con exito." -ForegroundColor Green
    exit 0
}

Write-Host "===========================================================" -ForegroundColor Cyan
Write-Host "  Instalador de Tlaxtrelom Engine v0.4.0-alpha.1 para Windows" -ForegroundColor Cyan
Write-Host "  Motor grafico de proxima generacion (2D..5D)" -ForegroundColor Cyan
Write-Host "===========================================================" -ForegroundColor Cyan

$CurrentDir = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "Instalando en: $InstallDir" -ForegroundColor White
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Copiar archivos
Copy-Item -Path "$CurrentDir\*" -Destination $InstallDir -Recurse -Force

# Crear acceso directo con WScript.Shell
$WshShell = New-Object -ComObject WScript.Shell

# Acceso directo en Escritorio
$DesktopLnkPath = "$env:USERPROFILE\Desktop\Tlaxtrelom.lnk"
$DesktopShortcut = $WshShell.CreateShortcut($DesktopLnkPath)
$DesktopShortcut.TargetPath = "$InstallDir\tlaxtrelom.cmd"
$DesktopShortcut.WorkingDirectory = $InstallDir
$DesktopShortcut.Description = "Tlaxtrelom Engine - Next-Gen Graphics Engine"
$DesktopShortcut.IconLocation = "$InstallDir\tlaxtrelom.exe,0"
$DesktopShortcut.Save()

# Acceso directo en Menu Inicio
$StartMenuDir = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Tlaxtrelom"
if (-not (Test-Path $StartMenuDir)) {
    New-Item -ItemType Directory -Path $StartMenuDir -Force | Out-Null
}
$StartMenuLnkPath = "$StartMenuDir\Tlaxtrelom.lnk"
$StartShortcut = $WshShell.CreateShortcut($StartMenuLnkPath)
$StartShortcut.TargetPath = "$InstallDir\tlaxtrelom.cmd"
$StartShortcut.WorkingDirectory = $InstallDir
$StartShortcut.Description = "Tlaxtrelom Engine"
$StartShortcut.IconLocation = "$InstallDir\tlaxtrelom.exe,0"
$StartShortcut.Save()

Write-Host "[OK] Accesos directos creados en Escritorio y Menu Inicio." -ForegroundColor Green
Write-Host "Tlaxtrelom Engine instalado correctamente." -ForegroundColor Green
Write-Host "Para ejecutar: inicia el acceso directo o ejecuta $InstallDir\tlaxtrelom.cmd" -ForegroundColor White
