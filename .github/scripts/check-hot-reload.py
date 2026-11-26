"""Drive disk edits and measure the first changed pixels in a game window."""

import ctypes as c
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "crates/asset/tests/fixtures"
SCENE = ROOT / "crates/app/examples/fixtures/hot-reload.scene.ron"
SHADERS = ROOT / "crates/render/src/scene"
BUDGET = 2.0
SHADER_BUDGET = 1.0


def find_marker(read_pixel, channel):
    def marker_at(x):
        rgb = read_pixel(x)
        value = rgb[channel]
        return value > 48 and value > 1.5 * max(rgb[:channel] + rgb[channel + 1:])

    return marker_at(160), any(marker_at(x) for x in range(225, 301, 5))


class X11Screen:
    name = "Xvfb"

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

    def marker_pixels(self, channel=0):
        # The Xvfb screen uses a 24-bit RGB visual. Capture the marker's midline.
        image = self.x11.XGetImage(self.display, self.root, 0, 120, 320, 1, 0xFFFFFF, 2)
        if not image:
            raise RuntimeError("could not capture the Xvfb frame")
        try:
            def read_pixel(x):
                pixel = self.x11.XGetPixel(image, x, 0)
                return (pixel >> 16) & 255, (pixel >> 8) & 255, pixel & 255

            return find_marker(read_pixel, channel)
        finally:
            self.x11.XDestroyImage(image)


class WindowsScreen:
    name = "Windows desktop"

    def __init__(self, pid):
        from ctypes import wintypes as w

        self.pid = pid
        self.types = w
        self.user = c.WinDLL("user32", use_last_error=True)
        self.user.SetProcessDpiAwarenessContext.argtypes = [w.HANDLE]
        self.user.SetProcessDpiAwarenessContext(w.HANDLE(-4))  # Per-monitor DPI aware v2.
        self.user.FindWindowW.argtypes = [w.LPCWSTR, w.LPCWSTR]
        self.user.FindWindowW.restype = w.HWND
        self.user.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
        self.user.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
        self.user.ClientToScreen.argtypes = [w.HWND, c.POINTER(w.POINT)]
        self.user.GetDC.argtypes = [w.HWND]
        self.user.GetDC.restype = w.HDC
        self.user.ReleaseDC.argtypes = [w.HWND, w.HDC]
        self.gdi = c.WinDLL("gdi32", use_last_error=True)
        self.gdi.GetPixel.argtypes = [w.HDC, c.c_int, c.c_int]
        self.gdi.GetPixel.restype = w.DWORD

    def close(self):
        pass  # Each capture releases its desktop DC.

    def marker_pixels(self, channel=0):
        window = self.user.FindWindowW(None, "rgine - hot reload")
        pid, bounds, origin = self.types.DWORD(), self.types.RECT(), self.types.POINT()
        if not window or not self.user.GetWindowThreadProcessId(window, c.byref(pid)):
            return False, False
        if pid.value != self.pid or not self.user.GetClientRect(window, c.byref(bounds)):
            return False, False
        if not self.user.ClientToScreen(window, c.byref(origin)):
            return False, False

        dc = self.user.GetDC(None)
        if not dc:
            raise c.WinError(c.get_last_error())
        try:
            # Read the composited desktop. Scale fixture coordinates into the client area.
            y = origin.y + bounds.bottom // 2

            def read_pixel(x):
                pixel = self.gdi.GetPixel(dc, origin.x + bounds.right * x // 320, y)
                return pixel & 255, (pixel >> 8) & 255, (pixel >> 16) & 255

            return find_marker(read_pixel, channel)
        finally:
            self.user.ReleaseDC(None, dc)


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
    until(process, time.monotonic() + 30, lambda: screen.marker_pixels() == (True, False),
          "the initial centered prop")

    replace(model_buffer, b"")
    until(process, time.monotonic() + 5,
          lambda: "truncated glTF buffer" in log.read_text(errors="replace"),
          "the failed import diagnostic")
    if screen.marker_pixels() != (True, False):
        raise RuntimeError("failed import changed the visible prop")

    moved = bytearray(original)
    for offset in range(0, 60, 12):
        x, = struct.unpack_from("<f", moved, offset)
        struct.pack_into("<f", moved, offset, x + 1.5)
    start = time.monotonic()
    replace(model_buffer, moved)
    visible = until(process, start + BUDGET, lambda: screen.marker_pixels() == (False, True),
                    "the edited external glTF buffer to appear")
    asset_elapsed = visible - start
    if asset_elapsed >= BUDGET:
        raise RuntimeError(f"asset reload took {asset_elapsed:.3f}s, budget is under {BUDGET:.1f}s")
    print(f"External glTF buffer edit to visible frame: {asset_elapsed * 1000:.1f} ms")

    replace(scene, b"(")
    until(process, time.monotonic() + 5,
          lambda: "could not read scene RON" in log.read_text(errors="replace"),
          "the invalid scene diagnostic")
    if screen.marker_pixels() != (False, True):
        raise RuntimeError("invalid scene changed the visible prop")

    start = time.monotonic()
    replace(scene, authored.replace(b"(0.0, 0.0, 0.0)", b"(-1.5, 0.0, 0.0)"))
    visible = until(process, start + BUDGET, lambda: screen.marker_pixels() == (True, False),
                    "the corrected scene translation to appear")
    print(f"Scene property edit to visible frame: {(visible - start) * 1000:.1f} ms")
    print("The previous frame survived both invalid edits; each correction became visible.")


def exercise_shader(process, screen, log, shader):
    original = shader.read_bytes()
    fragment = b"return lit_color(input, textureSample(material_texture, material_sampler, input.uv));"
    if original.count(fragment) != 1:
        raise RuntimeError("classic shader fixture does not contain the expected fragment return")
    edited = original.replace(fragment, b"return vec4<f32>(0.0, 1.0, 0.0, 1.0);")

    replace(shader, b"invalid wgsl")
    until(process, time.monotonic() + 5,
          lambda: f"{shader.name}: invalid scene shader" in log.read_text(errors="replace"),
          "the shader compilation diagnostic")
    if screen.marker_pixels() != (True, False):
        raise RuntimeError("invalid shader changed the visible prop")

    start = time.monotonic()
    replace(shader, edited)
    visible = until(process, start + SHADER_BUDGET, lambda: screen.marker_pixels(1)[0],
                    "the corrected shader to render the green prop")
    elapsed = visible - start
    if elapsed >= SHADER_BUDGET:
        raise RuntimeError(f"shader reload took {elapsed:.3f}s, budget is under {SHADER_BUDGET:.1f}s")
    runner = os.environ.get("RUNNER_NAME", "local")
    image = os.environ.get("ImageOS", platform.platform())
    backend = os.environ.get("WGPU_BACKEND", "auto")
    print(f"Shader edit to visible frame: {elapsed * 1000:.1f} ms "
          f"({runner}, {image}, {platform.machine()}, {backend}, {screen.name})")


def main():
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="rgine-hot-reload-") as directory:
        work = Path(directory)
        model = work / "marker.gltf"
        model_buffer = work / "marker.bin"
        scene = work / "level.scene.ron"
        shaders = work / "shaders"
        log = work / "game.log"
        model.write_bytes((ASSETS / model.name).read_bytes())
        model_buffer.write_bytes((ASSETS / model_buffer.name).read_bytes())
        scene.write_bytes(SCENE.read_bytes())
        shaders.mkdir()
        for shader in SHADERS.glob("*.wgsl"):
            shutil.copyfile(shader, shaders / shader.name)
        runtime = work / "runtime"
        runtime.mkdir(mode=0o700)
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime))
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("WAYLAND_SOCKET", None)
        with log.open("wb") as output:
            process = subprocess.Popen([binary, scene, model, "--shaders", shaders], stdout=output,
                                       stderr=subprocess.STDOUT, env=env)
            screen = None
            try:
                screen = WindowsScreen(process.pid) if os.name == "nt" else X11Screen()
                exercise(process, screen, log, model_buffer, scene)
                exercise_shader(process, screen, log, shaders / "primitive_classic.wgsl")
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
                if screen is not None:
                    screen.close()


if __name__ == "__main__":
    main()
