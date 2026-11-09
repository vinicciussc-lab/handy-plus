@echo off
rem Handy Plus: build environment for Windows.
rem - Loads the MSVC x64 tools and uses Ninja for CMake-based native deps
rem   (the Visual Studio generator fails here with MSBuild MSB1009).
rem - Points transcribe-cpp-sys at the engine built by scripts\build-transcribe.cmd,
rem   because its own cargo-driven CMake build fails on this machine.
rem Usage: scripts\dev-env.cmd <command...>
set "PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer;%PATH%"
call "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
set "PATH=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;%USERPROFILE%\.cargo\bin;%USERPROFILE%\.bun\bin;%PATH%"
for /d %%d in ("%LOCALAPPDATA%\Microsoft\WinGet\Packages\Oven-sh.Bun_*") do if exist "%%d\bun-windows-x64\bun.exe" set "PATH=%%d\bun-windows-x64;%PATH%"
set CMAKE_GENERATOR=Ninja
if "%VULKAN_SDK%"=="" for /f "tokens=2,*" %%a in ('reg query "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment" /v VULKAN_SDK 2^>nul ^| "%SystemRoot%\System32\find.exe" "VULKAN_SDK"') do set "VULKAN_SDK=%%b"
if exist "%~dp0..\.native\transcribe\lib\transcribe-link.json" set "TRANSCRIBE_DIR=%~dp0..\.native\transcribe"
%*
