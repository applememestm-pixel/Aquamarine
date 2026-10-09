@echo off
REM Copies the old Java server sources to a short path so Claude can read them
REM (the remote file bridge only reaches 7 folder levels deep).
robocopy "D:\pipo-server\src\main\java\ru\pipo\server" "D:\pipo-srv\java" *.java /S
robocopy "D:\pipo-server\src\main\resources" "D:\pipo-srv\resources" /S
robocopy "D:\pipo-server\config" "D:\pipo-srv\config" server-mode.properties
copy /Y "D:\pipo-server\pom.xml" "D:\pipo-srv\pom.xml" >nul
echo Done. Tell Claude: "D:\pipo-srv is ready".
pause
