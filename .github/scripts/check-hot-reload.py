"""Drive disk edits and measure the first changed pixel in an Xvfb game window."""

import ctypes as c
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "crates/asset/tests/fixtures"
SCENE = ROOT / "crates/app/examples/fixtures/hot-reload.scene.ron"
BUDGET = 2.0


class Screen:
    def __init__(self):
        self.x11 = c.CDLL("libX11.so.6")
        self.x11.XOpenDisplay.argtypes = [c.c_char_p]
        self.x11.XOpenDisplay.restype = c.c_void_p
        self.x11.XDefaultRootWindow.argtypes = [c.c_void_p]
        self.x11.XDefaultRootWindow.restype = c.c_ulong
        self.x11.XGetImage.argtypes = [
            c.c_void_p, c.c_ulong, c.c_int, c.c_int, c.c_uint, c.c_uint,
            c.c_ulong, c.c_int,
        ]
        self.x11.XGetImage.restype = c.c_void_p
        self.x11.XGetPixel.argtypes = [c.c_void_p, c.c_int, c.c_int]
        self.x11.XGetPixel.restype = c.c_ulong
        self.x11.XDestroyImage.argtypes = [c.c_void_p]
        self.x11.XCloseDisplay.argtypes = [c.c_void_p]
        self.display = self.x11.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError("could not open the Xvfb display")
        self.root = self.x11.XDefaultRootWindow(self.display)

    def close(self):
        self.x11.XCloseDisplay(self.display)

    def red_pixels(self):
        # The Xvfb screen uses a 24-bit RGB visual. Capture the marker's midline.
        image = self.x11.XGetImage(self.display, self.root, 0, 120, 320, 1, 0xFFFFFF, 2)
        if not image:
            raise RuntimeError("could not capture the Xvfb frame")
        try:
            def red_at(x):
                pixel = self.x11.XGetPixel(image, x, 0)
                red, green, blue = (pixel >> 16) & 255, (pixel >> 8) & 255, pixel & 255
                return red > 48 and red > 1.5 * max(green, blue)

            return red_at(160), any(red_at(x) for x in range(225, 301, 5))
        finally:
            self.x11.XDestroyImage(image)


def replace(path, contents):
    staged = path.with_suffix(".tmp")
    staged.write_bytes(contents)
    os.replace(staged, path)


def until(process, deadline, predicate, description):
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"game exited with code {process.returncode}")
        if predicate():
            return time.monotonic()
        time.sleep(0.01)
    raise RuntimeError(f"timed out waiting for {description}")


def exercise(process, screen, log, model_buffer, scene):
    original = model_buffer.read_bytes()
    authored = scene.read_bytes()
    until(process, time.monotonic() + 30, lambda: screen.red_pixels() == (True, False),
          "the initial centered prop")

    replace(model_buffer, b"")
    until(process, time.monotonic() + 5,
          lambda: "truncated glTF buffer" in log.read_text(errors="replace"),
          "the failed import diagnostic")
    if screen.red_pixels() != (True, False):
        raise RuntimeError("failed import changed the visible prop")

    moved = bytearray(original)
    for offset in range(0, 60, 12):
        x, = struct.unpack_from("<f", moved, offset)
        struct.pack_into("<f", moved, offset, x + 1.5)
    start = time.monotonic()
    replace(model_buffer, moved)
    visible = until(process, start + BUDGET, lambda: screen.red_pixels() == (False, True),
                    "the edited external glTF buffer to appear")
    asset_elapsed = visible - start
    if asset_elapsed >= BUDGET:
        raise RuntimeError(f"asset reload took {asset_elapsed:.3f}s, budget is under {BUDGET:.1f}s")
    print(f"External glTF buffer edit to visible frame: {asset_elapsed * 1000:.1f} ms")

    replace(scene, b"(")
    until(process, time.monotonic() + 5,
          lambda: "could not read scene RON" in log.read_text(errors="replace"),
          "the invalid scene diagnostic")
    if screen.red_pixels() != (False, True):
        raise RuntimeError("invalid scene changed the visible prop")

    start = time.monotonic()
    replace(scene, authored.replace(b"(0.0, 0.0, 0.0)", b"(-1.5, 0.0, 0.0)"))
    visible = until(process, start + BUDGET, lambda: screen.red_pixels() == (True, False),
                    "the corrected scene translation to appear")
    print(f"Scene property edit to visible frame: {(visible - start) * 1000:.1f} ms")
    print("The previous frame survived both invalid edits; each correction became visible.")


def main():
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="rgine-hot-reload-") as directory:
        work = Path(directory)
        model = work / "marker.gltf"
        model_buffer = work / "marker.bin"
        scene = work / "level.scene.ron"
        log = work / "game.log"
        model.write_bytes((ASSETS / model.name).read_bytes())
        model_buffer.write_bytes((ASSETS / model_buffer.name).read_bytes())
        scene.write_bytes(SCENE.read_bytes())
        runtime = work / "runtime"
        runtime.mkdir(mode=0o700)
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime))
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("WAYLAND_SOCKET", None)
        screen = Screen()
        try:
            with log.open("wb") as output:
                process = subprocess.Popen([binary, scene, model], stdout=output,
                                           stderr=subprocess.STDOUT, env=env)
                try:
                    exercise(process, screen, log, model_buffer, scene)
                except Exception:
                    print(log.read_text(errors="replace"), file=sys.stderr)
                    raise
                finally:
                    if process.poll() is None:
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
        finally:
            screen.close()


if __name__ == "__main__":
    main()
