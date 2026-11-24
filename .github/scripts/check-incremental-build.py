"""Check the Phase 2 incremental gameplay build budget in a CI checkout."""

import statistics
import subprocess
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/app/examples/procedural_scene.rs"
BUILD = ["cargo", "build", "--locked", "-p", "engine-app", "--example", "procedural_scene"]
TITLE = b'WindowDesc::new("rgine - procedural scene")'
BUDGET_SECONDS = 10.0


def main():
    original = SOURCE.read_bytes()
    if original.count(TITLE) != 1:
        raise SystemExit("Expected one window title in the procedural scene example")

    subprocess.run(BUILD, cwd=ROOT, check=True)
    samples = []
    try:
        for run in range(1, 4):
            edited_title = f'WindowDesc::new("rgine - procedural scene {run}")'.encode("ascii")
            SOURCE.write_bytes(original.replace(TITLE, edited_title))
            start = time.perf_counter()
            subprocess.run([*BUILD, "--offline"], cwd=ROOT, check=True)
            elapsed = time.perf_counter() - start
            samples.append(elapsed)
            print(f"Incremental gameplay build {run} took {elapsed:.3f} s", flush=True)
    finally:
        SOURCE.write_bytes(original)

    median = statistics.median(samples)
    print(f"Median incremental gameplay build took {median:.3f} s", flush=True)
    if median >= BUDGET_SECONDS:
        raise SystemExit(f"Median incremental gameplay build must be under {BUDGET_SECONDS:g} s")


if __name__ == "__main__":
    main()
