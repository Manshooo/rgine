"""Build and sign a local NativeActivity APK from the platformer library."""

import argparse
import os
from pathlib import Path
import platform
import subprocess
import tempfile
from zipfile import ZIP_DEFLATED, ZipFile


TARGETS = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}
HOSTS = {"Windows": "windows-x86_64", "Linux": "linux-x86_64", "Darwin": "darwin-x86_64"}
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def run(*args, env=None):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("abi", choices=TARGETS)
    parser.add_argument("--sdk", default=os.getenv("ANDROID_HOME") or os.getenv("ANDROID_SDK_ROOT"))
    parser.add_argument("--ndk", default=os.getenv("ANDROID_NDK_HOME"))
    parser.add_argument("--build-tools", default="35.0.0")
    parser.add_argument("--target-dir", default=os.getenv("CARGO_TARGET_DIR") or ROOT / "target")
    args = parser.parse_args()
    if not args.sdk or platform.system() not in HOSTS:
        parser.error("use a supported desktop host and set ANDROID_HOME or --sdk")

    sdk = Path(args.sdk).resolve()
    ndk = Path(args.ndk).resolve() if args.ndk else sdk / "ndk" / "27.1.12297006"
    host = platform.system()
    tools = sdk / "build-tools" / args.build_tools
    exe, bat = (".exe", ".bat") if host == "Windows" else ("", "")
    clang = ndk / "toolchains" / "llvm" / "prebuilt" / HOSTS[host] / "bin" / f"clang{exe}"
    android_jar = sdk / "platforms" / "android-35" / "android.jar"
    aapt2 = tools / f"aapt2{exe}"
    apksigner = tools / f"apksigner{bat}"
    for dependency in (clang, android_jar, aapt2, apksigner):
        if not dependency.is_file():
            parser.error(f"missing Android SDK tool or platform at {dependency}")

    target = TARGETS[args.abi]
    target_dir = Path(args.target_dir).resolve()
    prefix = "CARGO_TARGET_" + target.upper().replace("-", "_")
    env = os.environ | {
        "CARGO_TARGET_DIR": str(target_dir),
        f"{prefix}_LINKER": str(clang),
        f"{prefix}_RUSTFLAGS": f"-Clink-arg=--target={target}24",
    }
    run("cargo", "rustc", "--locked", "-p", "platformer-3d", "--lib", "--target", target,
        "--crate-type", "cdylib", env=env)

    library = target_dir / target / "debug" / "libplatformer_3d.so"
    output = target_dir / "platformer-android"
    output.mkdir(parents=True, exist_ok=True)
    key = output / "debug.keystore"
    if not key.exists():
        run("keytool", "-genkeypair", "-keystore", key, "-alias", "androiddebugkey",
            "-storepass", "android", "-keypass", "android", "-dname", "CN=Android Debug",
            "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000")

    apk = output / f"platformer-{args.abi}.apk"
    with tempfile.TemporaryDirectory(dir=output) as temp:
        unsigned = Path(temp) / "unsigned.apk"
        run(aapt2, "link", "-I", android_jar, "--manifest", HERE / "AndroidManifest.xml", "-o", unsigned)
        with ZipFile(unsigned, "a", ZIP_DEFLATED) as archive:
            archive.write(library, f"lib/{args.abi}/{library.name}")
        run(apksigner, "sign", "--ks", key, "--ks-pass", "pass:android",
            "--key-pass", "pass:android", "--out", apk, unsigned)
    run(apksigner, "verify", apk)
    print(apk)


if __name__ == "__main__":
    main()
