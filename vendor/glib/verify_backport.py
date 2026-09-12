"""Verify the complete glib source against its checksum-pinned crates.io archive."""

import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import tarfile
import urllib.request

ARCHIVE_URL = "https://static.crates.io/crates/glib/glib-0.18.5.crate"
ARCHIVE_SHA256 = "233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5"
MAX_ARCHIVE_BYTES = 4 * 1024 * 1024
ADDED_FILES = {
    ".gitattributes", "BACKPORT.json", "Cargo.lock", "verify_backport.py",
    "test_verify_backport.py", "tests/variant_str_iter_backport.rs",
}
MODIFIED_FILES = {"src/variant_iter.rs", "src/collections/strv.rs"}


def require(condition, message):
    """Integrity checks must remain enabled under Python -O/PYTHONOPTIMIZE."""
    if not condition:
        raise ValueError(message)


def source_file(vendor, relative):
    relative = PurePosixPath(relative)
    require(not relative.is_absolute() and ".." not in relative.parts, "Unsafe source path")
    path = vendor
    for part in relative.parts:
        path = path / part
        require(not path.is_symlink(), f"Source symlinks are not allowed: {relative}")
    require(path.resolve().is_relative_to(vendor), "Source path escapes vendor directory")
    require(path.is_file(), f"Missing source file: {relative}")
    return path


def expected_source(relative, original):
    if relative == "src/variant_iter.rs":
        before = b"let p: *mut libc::c_char = std::ptr::null_mut();"
        after = b"let mut p: *mut libc::c_char = std::ptr::null_mut();"
        require(original.count(before) == 1, "Unexpected upstream pointer declaration")
        require(original.count(b"                &p,") == 1, "Unexpected upstream pointer argument")
        return original.replace(before, after).replace(
            b"                &p,", b"                &mut p,"
        )
    if relative == "src/collections/strv.rs":
        before = b"assert!(s.get_unchecked(4).is_null());"
        after = b"assert!((*s.as_ptr().add(4)).is_null());"
        require(original.count(before) == 5, "Unexpected upstream sentinel tests")
        patched = original.replace(before, after)
        require(
            original.split(b"#[cfg(test)]")[0] == patched.split(b"#[cfg(test)]")[0],
            "The sentinel test repair must not change production code",
        )
        return patched
    return original


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="Previously downloaded official .crate")
    arguments = parser.parse_args()
    if arguments.archive:
        require(arguments.archive.is_file(), "Archive must be a regular file")
        with arguments.archive.open("rb") as source:
            archive_bytes = source.read(MAX_ARCHIVE_BYTES + 1)
    else:
        with urllib.request.urlopen(ARCHIVE_URL, timeout=30) as response:
            archive_bytes = response.read(MAX_ARCHIVE_BYTES + 1)
    require(len(archive_bytes) <= MAX_ARCHIVE_BYTES, "Archive exceeds size limit")
    require(hashlib.sha256(archive_bytes).hexdigest() == ARCHIVE_SHA256, "Archive mismatch")
    vendor = Path(__file__).resolve().parent
    manifest_path = source_file(vendor, "BACKPORT.json")
    require(manifest_path.stat().st_size <= 16 * 1024, "Backport manifest exceeds size limit")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    require(manifest["archive_sha256"] == ARCHIVE_SHA256, "Manifest archive mismatch")
    require(set(manifest["added_files"]) == ADDED_FILES, "Unexpected added files in manifest")
    require(set(manifest["modified_upstream_files"]) == MODIFIED_FILES, "Unexpected source patches")
    original_files = set()
    with tarfile.open(fileobj=io.BytesIO(archive_bytes)) as archive:
        for member in archive.getmembers():
            require(member.isfile() or member.isdir(), "Unexpected archive entry")
            if not member.isfile():
                continue
            relative = PurePosixPath(member.name).relative_to("glib-0.18.5")
            require(".." not in relative.parts, "Unsafe archive path")
            relative = relative.as_posix()
            original_files.add(relative)
            original = archive.extractfile(member).read()
            expected = expected_source(relative, original)
            path = source_file(vendor, relative)
            require(path.stat().st_size == len(expected), f"Source size mismatch: {relative}")
            actual = path.read_bytes()
            require(actual == expected, f"Unexpected source modification: {relative}")
            if relative in manifest["modified_upstream_files"]:
                hashes = manifest["modified_upstream_files"][relative]
                require(hashlib.sha256(original).hexdigest() == hashes["original_sha256"], "Original source hash mismatch")
                require(hashlib.sha256(actual).hexdigest() == hashes["patched_sha256"], "Patched source hash mismatch")
    require(len(original_files) == manifest["original_file_count"] == 121, "Upstream inventory mismatch")
    local_files = set()
    for path in vendor.rglob("*"):
        require(not path.is_symlink(), f"Source symlinks are not allowed: {path.name}")
        if path.is_file():
            local_files.add(path.relative_to(vendor).as_posix())
    require(local_files == original_files | ADDED_FILES, "File inventory mismatch")
    print("glib 0.18.5: 121 upstream files verified; production patch and test repair match")


if __name__ == "__main__":
    main()
