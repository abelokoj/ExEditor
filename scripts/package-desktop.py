"""Package an already-built native binary. This script runs only on GitHub runners."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from datetime import datetime, timezone


def package(target, commit):
    project = Path(__file__).resolve().parent.parent
    binary = project / "target/release/exeditor"
    if not binary.is_file():
        raise SystemExit("The GitHub-built application is missing.")
    if len(commit) != 40 or any(ch not in "0123456789abcdef" for ch in commit):
        raise SystemExit("A complete source commit is required.")
    output = project / "artifacts/desktop"
    output.mkdir(parents=True, exist_ok=True)
    version = tomllib.loads((project / "Cargo.toml").read_text())["package"]["version"]
    with tempfile.TemporaryDirectory(prefix="exeditor-package-") as temporary:
        staging = Path(temporary)
        if target.startswith("macos"):
            bundle = staging / "ExEditor.app"
            executable = bundle / "Contents/MacOS/exeditor"
            resources = bundle / "Contents/Resources"
            executable.parent.mkdir(parents=True)
            resources.mkdir(parents=True)
            shutil.copy2(binary, executable)
            executable.chmod(0o755)
            info = {
                "CFBundleName": "ExEditor", "CFBundleDisplayName": "ExEditor",
                "CFBundleIdentifier": "com.abelokoj.ExEditor", "CFBundleExecutable": "exeditor",
                "CFBundlePackageType": "APPL", "CFBundleShortVersionString": version,
                "CFBundleVersion": version, "CFBundleIconFile": "ExEditor.icns",
                "LSMinimumSystemVersion": "11.0", "NSHighResolutionCapable": True,
                "NSSupportsAutomaticGraphicsSwitching": True,
                "CFBundleDocumentTypes": [{"CFBundleTypeName": "Text and rich documents", "CFBundleTypeRole": "Editor",
                    "LSHandlerRank": "Alternate", "CFBundleTypeExtensions": ["txt", "rtf", "rtfd", "md", "markdown", "html", "htm", "doc", "docx", "odt"],
                    "LSItemContentTypes": ["public.text", "public.rtf", "public.html", "org.openxmlformats.wordprocessingml.document", "org.oasis-open.opendocument.text"]}],
            }
            (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
            icons = staging / "ExEditor.iconset"
            icons.mkdir()
            for size in [16, 32, 128, 256, 512]:
                for scale in [1, 2]:
                    name = f"icon_{size}x{size}" + ("@2x" if scale == 2 else "") + ".png"
                    subprocess.run(["/usr/bin/sips", "-z", str(size*scale), str(size*scale), str(project/"assets/ExEditor.png"), "--out", str(icons/name)], check=True, capture_output=True)
            subprocess.run(["/usr/bin/iconutil", "-c", "icns", str(icons), "-o", str(resources/"ExEditor.icns")], check=True)
            root = bundle
        else:
            root = staging / "ExEditor"
            root.mkdir()
            resources = root
            shutil.copy2(binary, root / "exeditor")
            (root / "exeditor").chmod(0o755)
            shutil.copy2(project/"assets/ExEditor.png", root/"ExEditor.png")
            (root/"install-desktop.py").write_text(INSTALL_DESKTOP, encoding="utf-8")
        for name in ["README.md", "CHANGELOG.md", "LICENSE", "Cargo.lock"]:
            shutil.copy2(project/name, resources/name)
        shutil.copy2(project/"docs/KEYBOARD_SHORTCUTS.txt", resources/"KEYBOARD_SHORTCUTS.txt")
        shutil.copytree(project/"assets/licenses", resources/"licenses")
        for font in ["inter", "pretendard-std", "maple-mono-nf", "noto-color-emoji"]:
            directory = resources/"licenses/fonts"/font
            directory.mkdir(parents=True, exist_ok=True)
            shutil.copy2(project/"assets/fonts"/font/"OFL.txt", directory/"OFL.txt")
        toolchain = tomllib.loads((project/"rust-toolchain.toml").read_text())["toolchain"]["channel"]
        info = {"application":"ExEditor", "version":version, "source_commit":commit,
                "target":target, "architecture":platform.machine(), "editor_engine":"WordCraft",
                "wordcraft_commit":"729cd0a5fc953bbedbfdec4f6af18ad6d1780b3b",
                "rust_toolchain":toolchain, "workflow_run":os.getenv("GITHUB_RUN_ID"),
                "built_at_utc":datetime.now(timezone.utc).isoformat()}
        (resources/"build-info.json").write_text(json.dumps(info, indent=2), encoding="utf-8")
        sums = [f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(root).as_posix()}" for path in sorted(root.rglob("*")) if path.is_file()]
        (resources/"SHA256SUMS.txt").write_text("\n".join(sums)+"\n", encoding="utf-8")
        if target.startswith("macos"):
            subprocess.run(["/usr/bin/codesign", "--force", "--deep", "--sign", "-", str(root)], check=True)
            subprocess.run(["/usr/bin/ditto", "-c", "-k", "--keepParent", str(root), str(output/"ExEditor-macos.zip")], check=True)
        else:
            with tarfile.open(output/"ExEditor-linux-x64.tar.gz", "w:gz") as archive:
                archive.add(root, arcname="ExEditor")


INSTALL_DESKTOP = r'''"""Register this extracted portable folder in the current user's Linux application menu."""
from pathlib import Path
import shutil
import subprocess

root=Path(__file__).resolve().parent
binary=str(root/"exeditor")
quoted='"'+binary.replace('\\','\\\\').replace('"','\\"').replace('$','\\$').replace('`','\\`').replace('%','%%')+'"'
desktop=Path.home()/".local/share/applications/exeditor.desktop"
desktop.parent.mkdir(parents=True,exist_ok=True)
desktop.write_text("[Desktop Entry]\\nType=Application\\nName=ExEditor\\nComment=Edit text and rich documents\\nTerminal=false\\nExec="+quoted+" %F\\nIcon="+str(root/"ExEditor.png")+"\\nMimeType=text/plain;text/markdown;text/html;application/rtf;application/msword;application/vnd.openxmlformats-officedocument.wordprocessingml.document;application/vnd.oasis.opendocument.text;\\nActions=Edit;\\n\\n[Desktop Action Edit]\\nName=Edit with ExEditor\\nExec="+quoted+" %F\\n",encoding="utf-8")
if shutil.which("update-desktop-database"):
    subprocess.run(["update-desktop-database",str(desktop.parent)],check=True)
print("Registered ExEditor from",root)
'''

if __name__ == "__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--target",required=True,choices=["linux-x64","macos-native"])
    parser.add_argument("--commit",required=True)
    args=parser.parse_args()
    package(args.target,args.commit)
