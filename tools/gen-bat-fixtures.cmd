@echo off
:: Regenerates crates\zapret\tests\fixtures from a Flowseal folder: a copy of every general*.bat
:: strategy and the "Final args" line that service.bat :service_install builds for it. The :parse
:: block is copied from service.bat, with %~dp0 replaced by %ROOT%.
:: Usage: tools\gen-bat-fixtures.cmd E:\Zapret
setlocal EnableDelayedExpansion
chcp 437 > nul
if "%~1"=="" (
    echo Usage: %~nx0 ^<zapret folder^>
    exit /b 1
)
for %%I in ("%~1") do set "ROOT=%%~fI"
if not "!ROOT:~-1!"=="\" set "ROOT=!ROOT!\"
set "OUT=%~dp0..\crates\zapret\tests\fixtures\"
if not exist "%OUT%" mkdir "%OUT%"
del /q "%OUT%*.bat" "%OUT%*.args" 2>nul
>"%OUT%ROOT.txt" echo !ROOT!

:: The game filter the Rust golden test uses: TCP and UDP on 1024-65535.
set "GameFilter=1024-65535"
set "GameFilterTCP=1024-65535"
set "GameFilterUDP=1024-65535"

:: Flowseal's strategies only: other .bat files in the folder may be the user's own scripts, and
:: Zapret Hub writes its own "general (HUB name).bat" files there, which must never become fixtures.
for %%F in ("%ROOT%general*.bat") do (
    set "name=%%~nF"
    if /i not "!name:~0,13!"=="general (HUB " (
        copy /y "%%~fF" "%OUT%" > nul
        call :parse "%%~fF" > "%OUT%%%~nF.args"
    )
)
echo Fixtures written to %OUT%
exit /b 0

:parse
set "selectedFile=%~1"
:: --- copied from service.bat :service_install ---
set "args_with_value=sni host altorder"
set "args="
set "capture=0"
set "mergeargs=0"
set "BIN=%ROOT%bin\"
set "LISTS=%ROOT%lists\"
set QUOTE="

for /f "tokens=*" %%a in ('type "!selectedFile!"') do (
    set "line=%%a"
    call set "line=%%line:^!=EXCL_MARK%%"
    call set "line=!line!"

    echo !line! | findstr /i "winws.exe" >nul
    if not errorlevel 1 (
        set "capture=1"
    )

    if !capture!==1 (
        if not defined args (
            set "line=!line:*winws.exe"=!"
        )

        set "temp_args="
        for %%i in (!line!) do (
            set "arg=%%i"

            if not "!arg!"=="^" if not "!arg!"=="^^" (
                if "!arg:~0,2!" EQU "--" if not !mergeargs!==0 (
                    set "mergeargs=0"
                )

                if "!arg:~0,1!" EQU "!QUOTE!" (
                    set "arg=!arg:~1,-1!"

                    echo !arg! | findstr ":" >nul
                    if !errorlevel!==0 (
                        set "arg=\!QUOTE!!arg!\!QUOTE!"
                    ) else if "!arg:~0,1!"=="@" (
                        set "arg=\!QUOTE!@%ROOT%!arg:~1!\!QUOTE!"
                    ) else (
                        set "arg=\!QUOTE!%ROOT%!arg!\!QUOTE!"
                    )
                )

                if !mergeargs!==1 (
                    set "temp_args=!temp_args!,!arg!"
                ) else if !mergeargs!==3 (
                    set "temp_args=!temp_args!=!arg!"
                    set "mergeargs=1"
                ) else (
                    set "temp_args=!temp_args! !arg!"
                )

                if "!arg:~0,2!" EQU "--" (
                    set "mergeargs=2"
                ) else if !mergeargs! GEQ 1 (
                    if !mergeargs!==2 set "mergeargs=1"

                    for %%x in (!args_with_value!) do (
                        if /i "%%x"=="!arg!" (
                            set "mergeargs=3"
                        )
                    )
                )
            )
        )

        if not "!temp_args!"=="" (
            set "args=!args! !temp_args!"
        )
    )
)

set ARGS=%args%
call set "ARGS=%%ARGS:EXCL_MARK=^!%%"
echo Final args: !ARGS!
exit /b
