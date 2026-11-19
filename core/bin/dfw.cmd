@echo off
rem dfw launcher for Windows (the sh launcher bin\dfw does the same elsewhere).
rem Runs the native dfw binary installed for this plugin version: <dfw home>\bin\<version>\dfw.exe,
rem where <dfw home> is DFW_HOME, else %XDG_CONFIG_HOME%\dfw, else %USERPROFILE%\.config\dfw.
rem DFW_BIN=<executable> runs that binary instead (tests and development).
setlocal EnableExtensions EnableDelayedExpansion
set "ROOT=%~dp0.."
for %%I in ("%ROOT%") do set "ROOT=%%~fI"
if defined DFW_BIN if exist "%DFW_BIN%" (
  set "DFW_PLUGIN_ROOT=%ROOT%"
  "%DFW_BIN%" %*
  exit /b !ERRORLEVEL!
)
set "MANIFEST=%ROOT%\.claude-plugin\plugin.json"
if not exist "%MANIFEST%" set "MANIFEST=%ROOT%\plugin.json"
set "VER="
if exist "%MANIFEST%" (
  for /f "tokens=2 delims=:," %%V in ('findstr /r /c:"\"version\"" "%MANIFEST%"') do if not defined VER set "VER=%%~V"
)
if defined VER set "VER=%VER: =%"
if defined VER set "VER=%VER:"=%"
if defined DFW_HOME (set "HOME_DIR=%DFW_HOME%") else if defined XDG_CONFIG_HOME (set "HOME_DIR=%XDG_CONFIG_HOME%\dfw") else (set "HOME_DIR=%USERPROFILE%\.config\dfw")
if defined VER if exist "%HOME_DIR%\bin\%VER%\dfw.exe" (
  set "DFW_PLUGIN_ROOT=%ROOT%"
  "%HOME_DIR%\bin\%VER%\dfw.exe" %*
  exit /b !ERRORLEVEL!
)
rem No binary for this plugin version: the newest installed one (folders sorted by version).
set "NEWEST="
if exist "%HOME_DIR%\bin" (
  for /f "delims=" %%D in ('dir /b /ad "%HOME_DIR%\bin" 2^>nul') do (
    if exist "%HOME_DIR%\bin\%%D\dfw.exe" call :newer "%%D"
  )
)
if defined NEWEST (
  echo dfw: no binary for plugin version %VER%; running %NEWEST%. Update: see the README ^(install^).>&2
  set "DFW_PLUGIN_ROOT=%ROOT%"
  "%HOME_DIR%\bin\%NEWEST%\dfw.exe" %*
  exit /b !ERRORLEVEL!
)
echo dfw: the dfw binary is not installed ^(looked in %HOME_DIR%\bin\%VER%\^).>&2
echo   Install it: download dfw-%VER%-win32-x64.exe from https://github.com/tantm97/ai-frontend-design-workflow/releases, check it against SHA256SUMS, save it as %HOME_DIR%\bin\%VER%\dfw.exe>&2
echo   ^(or, with Node.js: npx github:tantm97/ai-frontend-design-workflow^)>&2
exit /b 127

rem :newer "<version>": keep the higher of NEWEST and the given version (major.minor.patch).
:newer
set "CAND=%~1"
if not defined NEWEST (set "NEWEST=%CAND%" & exit /b 0)
for /f "tokens=1-3 delims=." %%a in ("%CAND%") do (set /a "C1=%%a+0, C2=%%b+0, C3=%%c+0" 2>nul)
for /f "tokens=1-3 delims=." %%a in ("%NEWEST%") do (set /a "N1=%%a+0, N2=%%b+0, N3=%%c+0" 2>nul)
if !C1! gtr !N1! (set "NEWEST=%CAND%" & exit /b 0)
if !C1! lss !N1! exit /b 0
if !C2! gtr !N2! (set "NEWEST=%CAND%" & exit /b 0)
if !C2! lss !N2! exit /b 0
if !C3! gtr !N3! set "NEWEST=%CAND%"
exit /b 0
