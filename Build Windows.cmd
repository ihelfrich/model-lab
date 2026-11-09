@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Install Rust stable and the Visual Studio C++ build tools, then rerun this file.
  pause
  exit /b 1
)
cargo test --locked
if errorlevel 1 exit /b 1
cargo build --release --locked
if errorlevel 1 exit /b 1
if not exist dist mkdir dist
if defined CARGO_TARGET_DIR (
  copy /y "%CARGO_TARGET_DIR%\release\tree-ensemble-lab.exe" "dist\Tree Ensemble Lab.exe"
) else (
  copy /y "target\release\tree-ensemble-lab.exe" "dist\Tree Ensemble Lab.exe"
)
if errorlevel 1 exit /b 1
echo Built dist\Tree Ensemble Lab.exe
rem Model Lab (the Tauri shell): engine-job sidecar, front end, NSIS installer.
python packaging\sidecar.py
if errorlevel 1 exit /b 1
python packaging\sidecar.py --check
if errorlevel 1 exit /b 1
pushd crates\app\ui
call npm ci
if errorlevel 1 exit /b 1
popd
pushd crates\app
call ui\node_modules\.bin\tauri.cmd build --ci
if errorlevel 1 exit /b 1
popd
echo Built target\release\bundle\nsis (Model Lab installer)
pause
