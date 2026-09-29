@echo off
cd /d "%~dp0"
py -3.14 src\app.py
if errorlevel 1 pause
