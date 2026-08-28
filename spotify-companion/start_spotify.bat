@echo off
cd /d "%~dp0"

if not exist .venv (
    py -m venv .venv
)

call .venv\Scripts\activate.bat
python -m pip install --disable-pip-version-check -r requirements.txt

if "%SPOTIPY_CLIENT_ID%"=="" (
    echo ERROR: SPOTIPY_CLIENT_ID is not set.
    pause
    exit /b 1
)

if "%SPOTIPY_REDIRECT_URI%"=="" (
    set SPOTIPY_REDIRECT_URI=http://127.0.0.1:8975/callback
)

python spotify_companion.py
