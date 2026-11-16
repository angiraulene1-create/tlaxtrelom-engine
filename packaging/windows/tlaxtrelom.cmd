@echo off
REM SPDX-License-Identifier: MIT
REM Copyright (c) 2026 angiraulene1-create
REM Arranque portable: el motor busca assets/ y shaders/ respecto al directorio
REM de trabajo, asi que nos situamos primero junto al ejecutable.
setlocal
cd /d "%~dp0"
tlaxtrelom.exe %*
set RC=%ERRORLEVEL%
endlocal & exit /b %RC%
