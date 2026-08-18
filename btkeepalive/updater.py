from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import threading
import tkinter as tk
import urllib.request
from collections.abc import Callable
from pathlib import Path
from tkinter import messagebox, ttk

from btkeepalive import __version__
from btkeepalive.app_log import log_error, log_info


def parse_version(version_str: str) -> tuple[int, ...]:
    """Parse version string like 'v1.2.0' or '1.3.1-beta' into an integer tuple."""
    clean = version_str.lstrip("v").strip()
    parts = []
    for part in clean.split("."):
        digit_part = ""
        for char in part:
            if char.isdigit():
                digit_part += char
            else:
                break
        if digit_part:
            parts.append(int(digit_part))
    return tuple(parts)


def cleanup_old_version() -> None:
    """Delete any .exe.old files in the executable's directory."""
    if not getattr(sys, "frozen", False):
        return
    try:
        exe_path = Path(sys.executable)
        old_exe = exe_path.with_suffix(".exe.old")
        if old_exe.is_file():
            old_exe.unlink()
            log_info("Cleaned up old executable: %s", old_exe.name)
    except Exception as exc:
        log_error("Failed to clean up old executable: %s", exc)


def get_latest_release(repo: str = "kadato/bt-keepalive") -> dict | None:
    """Fetch the latest release information from GitHub API."""
    url = f"https://api.github.com/repos/{repo}/releases/latest"
    req = urllib.request.Request(
        url,
        headers={"User-Agent": "BTKeepAlive-Updater"},
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            if response.status == 200:
                return json.loads(response.read().decode("utf-8"))
    except Exception as exc:
        log_error("Failed to fetch latest release from GitHub: %s", exc)
    return None


def fetch_expected_sha256(url: str) -> str | None:
    """Download SHA256SUMS.txt and extract the expected hash for BTKeepAlive.exe."""
    req = urllib.request.Request(url, headers={"User-Agent": "BTKeepAlive-Updater"})
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            content = response.read().decode("utf-8")
            for line in content.splitlines():
                parts = line.strip().split()
                if len(parts) >= 2 and parts[1] == "BTKeepAlive.exe":
                    return parts[0].strip().lower()
    except Exception as exc:
        log_error("Failed to fetch expected SHA256 checksum: %s", exc)
    return None


def compute_sha256(file_path: Path) -> str:
    """Compute the SHA256 checksum of a file."""
    sha = hashlib.sha256()
    with file_path.open("rb") as f:
        while True:
            chunk = f.read(65536)
            if not chunk:
                break
            sha.update(chunk)
    return sha.hexdigest()


def download_file(
    url: str,
    dest_path: Path,
    progress_callback: Callable[[int, int], None],
    check_cancelled: Callable[[], bool],
) -> bool:
    """Download a file with progress reporting and cancellation checks."""
    req = urllib.request.Request(url, headers={"User-Agent": "BTKeepAlive-Updater"})
    try:
        with urllib.request.urlopen(req, timeout=15) as response:
            total_size = int(response.headers.get("content-length", 0))
            downloaded = 0
            with dest_path.open("wb") as f:
                while True:
                    if check_cancelled():
                        return False
                    chunk = response.read(65536)
                    if not chunk:
                        break
                    f.write(chunk)
                    downloaded += len(chunk)
                    progress_callback(downloaded, total_size)
            return True
    except Exception as exc:
        log_error("Download failed: %s", exc)
        raise exc


def apply_hot_swap(new_path: Path) -> None:
    """Perform the renaming hot-swap and launch the new executable."""
    exe_path = Path(sys.executable)
    old_path = exe_path.with_suffix(".exe.old")
    try:
        # 1. Clean up any existing .exe.old just in case
        if old_path.is_file():
            try:
                old_path.unlink()
            except Exception:
                pass

        # 2. Rename active executable to .exe.old
        if not exe_path.is_file():
            raise FileNotFoundError(
                "The application executable has been moved or deleted since startup. "
                "Please restore the file or restart the application."
            )
        exe_path.rename(old_path)
        log_info("Renamed active exe to: %s", old_path.name)

        # 3. Rename .exe.new to active executable name
        new_path.rename(exe_path)
        log_info("Placed new exe at: %s", exe_path.name)

        # 4. Spawn the new executable as a detached process with a clean environment
        # to prevent PyInstaller path/temporary folder inheritance issues.
        env = os.environ.copy()
        env["PYINSTALLER_RESET_ENVIRONMENT"] = "1"
        env.pop("_MEIPASS", None)
        env.pop("_MEIPASS2", None)

        DETACHED_PROCESS = 0x00000008
        subprocess.Popen(
            [str(exe_path)],
            creationflags=DETACHED_PROCESS,
            close_fds=True,
            start_new_session=True,
            env=env,
        )
        log_info("Spawned new process: %s", exe_path.name)

        # 5. Exit parent process immediately
        os._exit(0)
    except Exception as exc:
        log_error("Hot swap failed: %s", exc)
        # Attempt recovery if possible
        try:
            if old_path.is_file() and not exe_path.is_file():
                old_path.rename(exe_path)
        except Exception:
            pass
        raise exc


class DownloadProgressDialog:
    def __init__(self, latest_ver: str, download_url: str, checksum_url: str) -> None:
        self.latest_ver = latest_ver
        self.download_url = download_url
        self.checksum_url = checksum_url
        self.root: tk.Tk | None = None
        self.progress: ttk.Progressbar | None = None
        self.progress_var: tk.DoubleVar | None = None
        self.status_var: tk.StringVar | None = None
        self.status_label: ttk.Label | None = None
        self.cancel_btn: ttk.Button | None = None
        self.cancelled = False
        self.success = False
        self.error_message: str | None = None

    def run(self) -> bool:
        self.root = tk.Tk()
        self.root.title("BT KeepAlive: Updating...")
        self.root.resizable(False, False)
        try:
            self.root.attributes("-topmost", True)
        except tk.TclError:
            pass

        # Setup Windows theme if available
        style = ttk.Style(self.root)
        for theme in ("vista", "winnative", "xpnative", "clam"):
            try:
                style.theme_use(theme)
                break
            except tk.TclError:
                continue

        main = ttk.Frame(self.root, padding=20)
        main.grid(row=0, column=0, sticky="nsew")

        ttk.Label(
            main,
            text=f"Downloading BT KeepAlive {self.latest_ver}...",
            font=("Segoe UI", 11, "bold"),
        ).grid(row=0, column=0, columnspan=2, sticky="w", pady=(0, 10))

        self.progress_var = tk.DoubleVar(value=0.0)
        self.progress = ttk.Progressbar(
            main,
            orient=tk.HORIZONTAL,
            length=350,
            mode="determinate",
            variable=self.progress_var,
        )
        self.progress.grid(row=1, column=0, columnspan=2, pady=(0, 10))

        self.status_var = tk.StringVar(value="Connecting...")
        self.status_label = ttk.Label(
            main, textvariable=self.status_var, foreground="#505050"
        )
        self.status_label.grid(row=2, column=0, sticky="w")

        self.cancel_btn = ttk.Button(main, text="Cancel", command=self.on_cancel)
        self.cancel_btn.grid(row=2, column=1, sticky="e")

        # Center dialog
        self.root.update_idletasks()
        w = self.root.winfo_reqwidth()
        h = self.root.winfo_reqheight()
        x = (self.root.winfo_screenwidth() - w) // 2
        y = (self.root.winfo_screenheight() - h) // 2
        self.root.geometry(f"+{x}+{y}")

        # Start download thread
        threading.Thread(target=self.download_worker, daemon=True).start()

        self.root.protocol("WM_DELETE_WINDOW", self.on_cancel)
        self.root.mainloop()

        return self.success

    def on_cancel(self) -> None:
        self.cancelled = True
        if self.status_var:
            self.status_var.set("Cancelling...")
        if self.root:
            self.root.destroy()

    def update_progress(self, downloaded: int, total: int) -> None:
        if self.status_var and self.progress_var:
            if total > 0:
                pct = (downloaded / total) * 100
                self.progress_var.set(pct)
                mb_downloaded = downloaded / (1024 * 1024)
                mb_total = total / (1024 * 1024)
                self.status_var.set(
                    f"Downloaded {mb_downloaded:.1f} MB of "
                    f"{mb_total:.1f} MB ({pct:.1f}%)"
                )
            else:
                self.status_var.set(f"Downloaded {downloaded / (1024 * 1024):.1f} MB")

    def download_worker(self) -> None:
        try:
            # 1. Fetch expected hash
            if self.status_var:
                self.status_var.set("Fetching checksum...")
            expected_hash = fetch_expected_sha256(self.checksum_url)
            if not expected_hash:
                raise Exception("Could not retrieve release checksums.")

            if self.cancelled:
                return

            # 2. Download executable
            if self.status_var:
                self.status_var.set("Downloading executable...")

            if not getattr(sys, "frozen", False):
                # When running from source, download to a temp location
                dest_dir = Path(__file__).resolve().parent.parent / "dist"
                dest_dir.mkdir(exist_ok=True)
                dest_path = dest_dir / "BTKeepAlive.exe.new"
            else:
                exe_path = Path(sys.executable)
                if not exe_path.is_file():
                    raise FileNotFoundError(
                        "The application executable has been moved "
                        "or deleted since startup. "
                        "Please restore the file or restart the application."
                    )
                dest_path = exe_path.with_suffix(".exe.new")

            def check_cancel() -> bool:
                return self.cancelled

            ok = download_file(
                self.download_url, dest_path, self.update_progress, check_cancel
            )
            if not ok or self.cancelled:
                if dest_path.is_file():
                    dest_path.unlink()
                return

            # 3. Verify checksum
            if self.status_var:
                self.status_var.set("Verifying integrity...")
            actual_hash = compute_sha256(dest_path)
            if actual_hash != expected_hash:
                if dest_path.is_file():
                    dest_path.unlink()
                raise Exception(
                    "SHA256 checksum verification failed.\nThe file may be corrupted."
                )

            if self.cancelled:
                if dest_path.is_file():
                    dest_path.unlink()
                return

            self.success = True
            # Close dialog on completion
            if self.root:
                self.root.after(0, self.root.destroy)

            # Apply hot swap if frozen
            if getattr(sys, "frozen", False):
                apply_hot_swap(dest_path)
            else:
                # Source mode notification
                self.root.after(
                    0,
                    lambda: messagebox.showinfo(
                        "BT KeepAlive: Update Completed",
                        "Downloaded and verified new version successfully "
                        "(saved in dist/BTKeepAlive.exe.new).\n\n"
                        "(Auto-restart skipped because you are running from source).",
                    ),
                )

        except Exception as exc:
            self.error_message = str(exc)
            if self.root:
                self.root.after(0, self.show_error)

    def show_error(self) -> None:
        messagebox.showerror(
            "BT KeepAlive: Update Failed",
            f"An error occurred during update:\n\n{self.error_message}",
        )
        if self.root:
            self.root.destroy()


def show_update_prompt(current_ver: str, latest_ver: str) -> bool:
    """Show a yes/no dialog prompting the user to update."""
    root = tk.Tk()
    root.withdraw()
    try:
        root.attributes("-topmost", True)
    except tk.TclError:
        pass
    msg = (
        f"A new version of BT KeepAlive ({latest_ver}) is available "
        f"(current: {current_ver}).\n\n"
        "Would you like to download and install the update now?"
    )
    ans = messagebox.askyesno(
        title="BT KeepAlive: Update Available",
        message=msg,
        parent=root,
    )
    root.destroy()
    return ans


def show_info_dialog(title: str, message: str) -> None:
    """Show a generic informational message box."""
    root = tk.Tk()
    root.withdraw()
    try:
        root.attributes("-topmost", True)
    except tk.TclError:
        pass
    messagebox.showinfo(title, message, parent=root)
    root.destroy()


def check_for_update_available() -> dict | None:
    """Check if an update is available on GitHub.

    Returns:
        dict: A dictionary with 'version', 'download_url', and
              'checksum_url' if available, otherwise None.
    """
    try:
        log_info("Checking for updates...")
        release = get_latest_release()
        if not release:
            log_info("Failed to check for updates (no release data).")
            return None

        tag_name = release.get("tag_name", "")
        if not tag_name:
            log_info("Invalid release details received from GitHub (no tag_name).")
            return None

        latest_ver = parse_version(tag_name)
        current_ver = parse_version(__version__)

        if latest_ver > current_ver:
            # Find the download and checksum URLs
            download_url = ""
            checksum_url = ""
            for asset in release.get("assets", []):
                name = asset.get("name", "")
                if name == "BTKeepAlive.exe":
                    download_url = asset.get("browser_download_url", "")
                elif name == "SHA256SUMS.txt":
                    checksum_url = asset.get("browser_download_url", "")

            if not download_url or not checksum_url:
                log_info("Could not locate required release assets on GitHub.")
                return None

            return {
                "version": tag_name,
                "download_url": download_url,
                "checksum_url": checksum_url,
            }

        log_info("BT KeepAlive is up to date (current: %s)", __version__)
    except Exception as exc:
        log_error("Error in check_for_update_available: %s", exc)
    return None


def run_update_install(version: str, download_url: str, checksum_url: str) -> None:
    """Run the download progress dialog and hot swap process."""
    dialog = DownloadProgressDialog(version, download_url, checksum_url)
    dialog.run()


def check_for_updates_workflow(*, manual: bool = False) -> None:
    """Run the update checking and installation workflow (compatibility fallback)."""

    def worker() -> None:
        details = check_for_update_available()
        if details:
            if show_update_prompt(__version__, details["version"]):
                run_update_install(
                    details["version"],
                    details["download_url"],
                    details["checksum_url"],
                )
        elif manual:
            # If manual check and no update was found or failed
            release = get_latest_release()
            if not release:
                msg = (
                    "Failed to check for updates.\n"
                    "Please verify your internet connection and try again."
                )
                show_info_dialog("BT KeepAlive: Update Check Failed", msg)
            else:
                tag_name = release.get("tag_name", "")
                if not tag_name:
                    show_info_dialog(
                        "BT KeepAlive: Update Check Failed",
                        "Invalid release details received from GitHub.",
                    )
                else:
                    latest_ver = parse_version(tag_name)
                    current_ver = parse_version(__version__)
                    if latest_ver <= current_ver:
                        msg = (
                            f"You are up to date! BT KeepAlive v{__version__} "
                            f"is the latest version."
                        )
                        show_info_dialog("BT KeepAlive: Up to Date", msg)

    threading.Thread(target=worker, name="update-checker", daemon=True).start()
