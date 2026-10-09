@echo off
title Aquamarine - bundle the reaction pack into the build
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0bundle-reactions.ps1"
pause
