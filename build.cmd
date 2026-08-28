@echo off
setlocal

where cl.exe >nul 2>nul
if errorlevel 1 (
  echo Run this from a "x64 Native Tools Command Prompt for Visual Studio".
  exit /b 1
)
where rc.exe >nul 2>nul
if errorlevel 1 (
  echo Windows SDK Resource Compiler rc.exe was not found.
  exit /b 1
)

set "OUT_DIR=out"
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"

rc.exe /nologo /fo "%OUT_DIR%\resources.res" resources.rc
if errorlevel 1 exit /b 1

cl.exe /nologo /std:c++17 /W4 /permissive- /O2 /MT /EHsc ^
  /DUNICODE /D_UNICODE ^
  /Fo"%OUT_DIR%\LastKey.obj" LastKey.cpp "%OUT_DIR%\resources.res" ^
  /link /SUBSYSTEM:WINDOWS /INCREMENTAL:NO ^
  /OUT:"%OUT_DIR%\LastKey.exe" user32.lib shell32.lib
if errorlevel 1 exit /b 1

echo.
echo Release build created: %OUT_DIR%\LastKey.exe
