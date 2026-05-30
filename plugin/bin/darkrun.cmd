@echo off
rem darkrun launcher (Windows) — locate and exec the native per-arch binary.
rem No JavaScript runtime; this only resolves the prebuilt darkrun.exe.
setlocal
set "PKG=%~dp0.."
if not defined CLAUDE_PLUGIN_ROOT set "CLAUDE_PLUGIN_ROOT=%PKG%"

set "ARCH=x64"
if /I "%PROCESSOR_ARCHITECTURE%"=="ARM64" set "ARCH=arm64"
set "PLAT=win32-%ARCH%"

set "C1=%PKG%\node_modules\@darkrun\%PLAT%\bin\darkrun.exe"
set "C2=%PKG%\..\@darkrun\%PLAT%\bin\darkrun.exe"
set "C3=%PKG%\..\target\release\darkrun.exe"
set "C4=%PKG%\..\target\debug\darkrun.exe"

if exist "%C1%" ( "%C1%" %* & exit /b %errorlevel% )
if exist "%C2%" ( "%C2%" %* & exit /b %errorlevel% )
if exist "%C3%" ( "%C3%" %* & exit /b %errorlevel% )
if exist "%C4%" ( "%C4%" %* & exit /b %errorlevel% )

echo darkrun: no native binary found for %PLAT%. Reinstall the npm package or run: cargo build --release -p darkrun-cli 1>&2
exit /b 1
