"""Package a native release build and its SHA-256 checksum."""

import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib


def main():
    tag, target = sys.argv[1:]
    version = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if tag != f"v{version}":
        sys.exit(f"Tag {tag} does not match Cargo.toml version {version}")

    executable = "teapot.exe" if "windows" in target else "teapot"
    binary = Path("target") / target / "release" / executable
    result = subprocess.run([str(binary.resolve()), "--version"], check=True, capture_output=True, text=True)
    if result.stdout.strip() != f"teapot {version}":
        sys.exit(f"Unexpected binary version: {result.stdout.strip()}")

    name = f"teapot-{tag}-{target}"
    output = Path("target/dist")
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temporary:
        package = Path(temporary) / name
        package.mkdir()
        shutil.copy2(binary, package / executable)
        shutil.copy2("README.md", package / "README.md")
        shutil.copy2("config.toml", package / "config.example.toml")
        archive = Path(shutil.make_archive(
            str(output / name), "zip" if "windows" in target else "gztar",
            root_dir=temporary, base_dir=name,
        ))

    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(
        f"{digest}  {archive.name}\n", encoding="utf-8"
    )
    print(f"Packaged {archive}")


if __name__ == "__main__":
    main()
