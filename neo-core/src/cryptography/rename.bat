@echo off
setlocal enabledelayedexpansion

REM Function: Convert camel case to snake case
:camel_to_snake
set "str=%~1"
set "result="
for /l %%i in (0,1,127) do (
    set "char=!str:~%%i,1!"
    if "!char!"=="" goto :done
    if "!char!" geq "A" if "!char!" leq "Z" (
        set "char=_!char!"
        set "char=!char:A=a!"
        set "char=!char:B=b!"
        set "char=!char:C=c!"
        set "char=!char:D=d!"
        set "char=!char:E=e!"
        set "char=!char:F=f!"
        set "char=!char:G=g!"
        set "char=!char:H=h!"
        set "char=!char:I=i!"
        set "char=!char:J=j!"
        set "char=!char:K=k!"
        set "char=!char:L=l!"
        set "char=!char:M=m!"
        set "char=!char:N=n!"
        set "char=!char:O=o!"
        set "char=!char:P=p!"
        set "char=!char:Q=q!"
        set "char=!char:R=r!"
        set "char=!char:S=s!"
        set "char=!char:T=t!"
        set "char=!char:U=u!"
        set "char=!char:V=v!"
        set "char=!char:W=w!"
        set "char=!char:X=x!"
        set "char=!char:Y=y!"
        set "char=!char:Z=z!"
    )
    set "result=!result!!char!"
)
:done
endlocal & set "%~2=%result%"
goto :eof

REM Iterate over all .cs files in the current directory
for %%f in (*.cs) do (
    REM Check if file exists
    if exist "%%f" (
        REM Get filename without extension
        set "filename=%%~nf"
        
        REM Convert to snake case
        call :camel_to_snake "%%filename%%" new_filename
        
        REM Rename file
        ren "%%f" "!new_filename!.rs"
        echo Renamed: %%f -> !new_filename!.rs
    )
)

echo Renaming completed.