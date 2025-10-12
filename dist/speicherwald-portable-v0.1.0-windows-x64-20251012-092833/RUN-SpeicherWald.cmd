@echo off
setlocal
:: Starte Server im Hintergrund und öffne Browser
start "" "%~dp0speicherwald.exe"
timeout /t 1 >nul
start "" http://127.0.0.1:8080/
