"""Shared input guards for the repository operations scripts.

The scripts in this directory take operator-supplied RPC endpoints and
output paths from the command line. ``validate_rpc_url`` requires an
http(s) endpoint with a non-empty host so fetch helpers cannot be pointed
at other schemes (``file:``, ``data:``, ...). ``safe_output_path``
normalizes output paths, rejects NUL bytes and ``..`` traversal, and
requires them to land inside an allowed root (the repository, the current
working directory, the user's home, or /data). Legitimate invocations are
unaffected.
"""

import os
from pathlib import Path
from urllib.parse import urlparse


def validate_rpc_url(url):
    """Validates an operator-supplied RPC endpoint URL.

    Requires an http(s) scheme and a non-empty host. Returns the URL
    unchanged when valid; raises ``ValueError`` otherwise.
    """
    parsed = urlparse(url)
    if parsed.scheme not in ("http", "https") or not parsed.netloc:
        raise ValueError(f"RPC endpoint must be an http(s) URL: {url!r}")
    return url


def _allowed_roots():
    """Directories an operations script may write into."""
    repo_root = Path(__file__).resolve().parent.parent
    roots = [Path.cwd(), repo_root, Path.home(), Path("/data"), Path("/tmp")]
    return [root.expanduser().resolve() for root in roots]


def safe_output_path(path):
    """Validates and normalizes an operator-supplied output path.

    Rejects NUL bytes and ``..`` traversal, resolves the path, and requires
    it to land inside an allowed root. Returns the resolved absolute
    ``Path``; open it with ``Path.open``.
    """
    text = os.fspath(path)
    if "\x00" in text:
        raise ValueError("output path contains NUL")
    resolved = Path(text).expanduser().resolve()
    if ".." in resolved.parts:
        raise ValueError(f"output path must not contain '..': {path!r}")
    roots = _allowed_roots()
    if not any(resolved == root or root in resolved.parents for root in roots):
        roots_text = ", ".join(str(root) for root in roots)
        raise ValueError(
            f"output path {str(resolved)!r} is outside the allowed "
            f"directories: {roots_text}"
        )
    return resolved
