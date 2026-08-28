import json
import os
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

import spotipy
from spotipy.exceptions import SpotifyException
from spotipy.oauth2 import SpotifyPKCE
from spotipy.cache_handler import CacheFileHandler

API_HOST = "127.0.0.1"
API_PORT = 8974
POLL_SECONDS = 15.0
REQUEST_TIMEOUT = 10
REQUEST_RETRIES = 0

REDIRECT_URI = os.environ.get(
    "SPOTIPY_REDIRECT_URI",
    "http://127.0.0.1:8975/callback",
)
SCOPE = "user-read-currently-playing user-read-playback-state"

CLIENT_ID = os.environ.get("SPOTIPY_CLIENT_ID", "").strip()
if not CLIENT_ID:
    raise RuntimeError("SPOTIPY_CLIENT_ID is not set.")

APPDATA_DIR = os.path.join(
    os.environ.get("LOCALAPPDATA", os.path.expanduser("~")),
    "Threepole",
)
os.makedirs(APPDATA_DIR, exist_ok=True)

CACHE_PATH = os.path.join(APPDATA_DIR, "spotify-pkce-cache")
LOG_PATH = os.path.join(APPDATA_DIR, "spotify-companion.log")

state_lock = threading.Lock()
now_playing = {
    "isPlaying": False,
    "track": None,
    "artist": None,
    "album": None,
    "progressMs": None,
    "durationMs": None,
    "error": "Waiting for first Spotify poll.",
    "updatedAt": None,
}


def log(message: str):
    stamp = time.strftime("%Y-%m-%d %H:%M:%S")
    try:
        with open(LOG_PATH, "a", encoding="utf-8") as f:
            f.write(f"[{stamp}] {message}\n")
            f.flush()
    except Exception:
        pass


def set_state(**values):
    with state_lock:
        now_playing.update(values)
        now_playing["updatedAt"] = int(time.time() * 1000)


def snapshot():
    with state_lock:
        return dict(now_playing)


cache_handler = CacheFileHandler(cache_path=CACHE_PATH)
auth_manager = SpotifyPKCE(
    client_id=CLIENT_ID,
    redirect_uri=REDIRECT_URI,
    scope=SCOPE,
    open_browser=True,
    cache_handler=cache_handler,
)

spotify = spotipy.Spotify(
    auth_manager=auth_manager,
    requests_timeout=REQUEST_TIMEOUT,
    retries=REQUEST_RETRIES,
)


def spotify_poll_loop():
    log("Spotify poll thread entered.")

    while True:
        try:
            log("Spotify poll: requesting currently-playing...")
            started = time.monotonic()

            playback = spotify.current_user_playing_track()

            elapsed = time.monotonic() - started
            log(f"Spotify poll: request returned after {elapsed:.2f}s.")

            item = (playback or {}).get("item")

            if not item:
                set_state(
                    isPlaying=False,
                    track=None,
                    artist=None,
                    album=None,
                    progressMs=None,
                    durationMs=None,
                    error=None,
                )
                log("Spotify poll: no current track.")
            else:
                track = item.get("name")
                artist = ", ".join(
                    a.get("name", "") for a in item.get("artists", [])
                )
                set_state(
                    isPlaying=bool((playback or {}).get("is_playing")),
                    track=track,
                    artist=artist,
                    album=(item.get("album") or {}).get("name"),
                    progressMs=(playback or {}).get("progress_ms"),
                    durationMs=item.get("duration_ms"),
                    error=None,
                )
                log(f"Spotify poll: updated cache with {track!r} by {artist!r}.")

            time.sleep(POLL_SECONDS)

        except SpotifyException as exc:
            if exc.http_status == 429:
                retry_after = 300
                headers = getattr(exc, "headers", None) or {}
                raw_retry = headers.get("Retry-After") or headers.get("retry-after")
                try:
                    retry_after = max(1, int(raw_retry))
                except (TypeError, ValueError):
                    pass

                message = f"Spotify rate limited. Retrying in {retry_after} seconds."
                set_state(error=message)
                log(message)
                time.sleep(retry_after)
            else:
                message = f"Spotify API error {exc.http_status}: {exc}"
                set_state(error=message)
                log(message)
                time.sleep(15)

        except Exception as exc:
            message = f"Spotify request failed: {type(exc).__name__}: {exc}"
            set_state(error=message)
            log(message)
            time.sleep(15)


class Handler(BaseHTTPRequestHandler):
    def _send_json(self, payload, status=200):
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")
        self.end_headers()

    def do_GET(self):
        path = urlparse(self.path).path

        if path == "/":
            self._send_json({
                "status": "ok",
                "service": "threepole-spotify-pkce",
                "pollSeconds": POLL_SECONDS,
                "requestTimeoutSeconds": REQUEST_TIMEOUT,
                "nowPlaying": f"http://{API_HOST}:{API_PORT}/now-playing",
            })
            return

        if path == "/now-playing":
            self._send_json(snapshot())
            return

        self._send_json({"error": "not found"}, 404)

    def log_message(self, *_):
        pass


try:
    httpd = ThreadingHTTPServer((API_HOST, API_PORT), Handler)
except OSError as exc:
    log(f"Another companion already owns port {API_PORT}: {exc}")
    raise SystemExit(0)

log("HTTP server bound successfully.")

try:
    auth_manager.get_access_token(check_cache=True)
except Exception as exc:
    log(f"Spotify authorization failed: {type(exc).__name__}: {exc}")
    httpd.server_close()
    raise

log("Spotify authorization ready.")

poll_thread = threading.Thread(
    target=spotify_poll_loop,
    name="spotify-poller",
    daemon=True,
)
poll_thread.start()

log(
    f"Spotify companion started; polling every {POLL_SECONDS:g}s "
    f"with {REQUEST_TIMEOUT}s request timeout."
)

try:
    httpd.serve_forever()
finally:
    httpd.server_close()
