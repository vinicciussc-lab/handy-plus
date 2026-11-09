@echo off
rem Handy Plus: build transcribe.cpp (the speech engine) outside cargo.
rem On this machine the cargo-driven CMake build fails at CMake's compiler
rem check (ninja/MSBuild cannot load files through the build script's
rem junction), while a direct CMake build works. This builds the same
rem configuration transcribe-cpp-sys 0.4.0 uses on Windows x64 and installs
rem it to .native\transcribe; scripts\dev-env.cmd points TRANSCRIBE_DIR there.
rem Usage: scripts\dev-env.cmd scripts\build-transcribe.cmd
setlocal
set "SRC=%USERPROFILE%\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\transcribe-cpp-sys-0.4.0"
set "BUILD=%LOCALAPPDATA%\tcs\pre"
set "PREFIX=%~dp0..\.native\transcribe"
if not exist "%SRC%\CMakeLists.txt" (echo Run "cargo fetch" in src-tauri first & exit /b 1)
cmake -S "%SRC%" -B "%BUILD%" -G Ninja -DCMAKE_BUILD_TYPE=Release ^
  -DTRANSCRIBE_INSTALL=ON -DTRANSCRIBE_BUILD_TESTS=OFF -DTRANSCRIBE_BUILD_EXAMPLES=OFF ^
  -DTRANSCRIBE_BUILD_TOOLS=OFF -DTRANSCRIBE_BUILD_SHARED=ON -DTRANSCRIBE_GGML_BACKEND_DL=ON ^
  -DGGML_CPU_ALL_VARIANTS=ON -DTRANSCRIBE_X86_CONSERVATIVE=ON -DTRANSCRIBE_VULKAN=ON ^
  -DTRANSCRIBE_USE_OPENMP=OFF "-DCMAKE_C_FLAGS=/O2 /Ob2 /DNDEBUG -nologo -MD -Brepro -W0" ^
  "-DCMAKE_CXX_FLAGS=/O2 /Ob2 /DNDEBUG -nologo -MD -Brepro -W0" || exit /b 1
cmake --build "%BUILD%" --config Release || exit /b 1
cmake --install "%BUILD%" --prefix "%PREFIX%" || exit /b 1
echo Installed to %PREFIX%
