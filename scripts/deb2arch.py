#!/usr/bin/env python3
"""
Convert a Tauri .deb into an Arch .pkg.tar.zst, without debtap.

Why not debtap: it is written to run *on Arch*. It shells out to `pkgfile` in
the conversion path (not just in `debtap -u`), hard-exits unless
/var/cache/pkgfile and a set of /var/cache/debtap/* databases exist, and calls
`namcap`. `pkgfile` and `namcap` are Arch packages with no Ubuntu equivalent,
so running debtap on the ubuntu-22.04 runner could never have worked — it was
written untested and failed on its first tag.

Doing it by hand is small because Tauri's .deb is a couple of small files with
three declared dependencies. A .pkg.tar.zst is just a zstd tar carrying
.PKGINFO and .MTREE alongside the file tree, all of which is more reliably
produced here than by translating Debian package names against an Arch
database we do not have.

The file tree is read straight out of data.tar rather than extracted to disk
first. That keeps each member's mode and mtime exactly as the .deb recorded
them — extracting and re-statting picks up whatever the host filesystem
invents for permissions, which is how you end up shipping a 777 binary.

Run: deb2arch.py <input.deb> <output-dir>
"""

import hashlib
import io
import os
import posixpath
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

# --- the part that needs human judgment ---------------------------------------
#
# What Tauri declares in the .deb, and what provides it on Arch. Everything
# else the binary links against arrives transitively through gtk3 or
# webkit2gtk-4.1.
DEP_MAP = {
    "libayatana-appindicator3-1": "libayatana-appindicator",
    "libwebkit2gtk-4.1-0": "webkit2gtk-4.1",
    "libgtk-3-0": "gtk3",
}

# The .deb never declares ALSA, but the binary links libasound.so.2 because the
# audio core (CPAL/alsa-sys) talks to ALSA directly. Tauri only lists the heavy
# runtime deps, so this is simply absent from its Depends, and on a minimal Arch
# install that is a hard launch failure. It is added conditionally, by looking
# for the soname, so it disappears on its own if the audio backend ever stops
# linking it.
SONAME_DEPS = {
    "libasound.so.2": "alsa-lib",
}

LICENSE = "MIT"
URL = "https://github.com/schrab/misima-hybrid-player"
PKGREL = "1"

ARCH_MAP = {"amd64": "x86_64", "arm64": "aarch64", "i386": "i686"}


def die(msg):
    print(f"deb2arch: {msg}", file=sys.stderr)
    sys.exit(1)


def ar_members(blob):
    """Split a Unix ar archive into {name: bytes}. Debs are always ar."""
    if blob[:8] != b"!<arch>\n":
        die("not a Debian archive (bad ar magic)")
    out, off = {}, 8
    while off < len(blob):
        hdr = blob[off:off + 60]
        if len(hdr) < 60:
            break
        name = hdr[0:16].decode().strip().rstrip("/")
        try:
            size = int(hdr[48:58].decode().strip())
        except ValueError:
            die(f"corrupt ar header for {name!r}")
        out[name] = blob[off + 60:off + 60 + size]
        off += 60 + size + (size % 2)
    return out


def tar_mode(name):
    if name.endswith((".tar.gz", ".tgz")):
        return "r:gz"
    if name.endswith((".tar.xz", ".txz")):
        return "r:xz"
    if name.endswith((".tar.zst", ".tzst")):
        return "r:zst"
    if name.endswith(".tar"):
        return "r:"
    die(f"unknown compression for {name!r}")


def open_tar_from_bytes(blob, hint, tmpdir, tag):
    """tarfile needs a real file for some compressed formats; give it one."""
    path = os.path.join(tmpdir, f"{tag}.tar")
    with open(path, "wb") as f:
        f.write(blob)
    try:
        return tarfile.open(path, tar_mode(hint))
    except tarfile.TarError as e:
        die(f"cannot read {tag}: {e}")


def parse_control(text):
    """Parse a deb control stanza. Only the fields we need, no folding games."""
    fields, key = {}, None
    for line in text.splitlines():
        if not line.strip():
            continue
        if line[0] in " \t":  # continuation of the previous field
            if key:
                fields[key] += " " + line.strip()
            continue
        if ":" not in line:
            continue
        key, _, val = line.partition(":")
        key = key.strip()
        fields[key] = val.strip()
    return fields


def mtree_quote(path):
    """mtree is whitespace-delimited; anything unusual has to be quoted."""
    if re.fullmatch(r"[A-Za-z0-9._/+-]+", path):
        return path
    return '"' + path.replace("\\", "\\\\").replace('"', '\\"') + '"'


def norm(name):
    """data.tar paths are sometimes './usr/...'; store them bare."""
    return posixpath.normpath(name).lstrip("/")


class Payload:
    """The deb's file tree, held in memory exactly as data.tar described it."""

    def __init__(self, tar):
        self.items = []  # (name, kind, mode, mtime, data|None)
        for m in tar.getmembers():
            if not (m.isfile() or m.isdir() or m.issym()):
                die(f"unsupported payload entry type: {m.name}")
            name = norm(m.name)
            if name == ".":
                continue
            data = tar.extractfile(m).read() if m.isfile() else None
            kind = "dir" if m.isdir() else ("link" if m.issym() else "file")
            self.items.append((name, kind, m.mode & 0o7777, int(m.mtime), data))
        self.items.sort(key=lambda i: i[0])

    def elf_blobs(self):
        for name, kind, _mode, _mt, data in self.items:
            if kind == "file" and data[:4] == b"\x7fELF":
                yield data


def resolve_depends(control, payload):
    """Arch depend= list: declared deps mapped, plus sonames the deb omits."""
    declared = [d.strip() for d in control.get("Depends", "").split(",") if d.strip()]
    depends = []
    for dep in declared:
        name = dep.split()[0].split(":")[0]  # drop any version constraint
        mapped = DEP_MAP.get(name)
        if mapped is None:
            die(
                f"deb depends on {name!r}, which has no entry in DEP_MAP.\n"
                "Add it to scripts/deb2arch.py — refusing to guess, a wrong "
                "name here means an uninstallable package."
            )
        if mapped not in depends:
            depends.append(mapped)

    blob = b"".join(payload.elf_blobs())
    for soname, pkg in SONAME_DEPS.items():
        if soname.encode() in blob and pkg not in depends:
            depends.append(pkg)
    return sorted(depends)


def build_mtree(payload, pkginfo_bytes, mtime):
    lines = ["#mtree", "/set type=file uid=0 gid=0 mode=644"]
    for name, kind, mode, mt, data in payload.items:
        path = mtree_quote("./" + name)
        if kind == "dir":
            lines.append(f"{path} type=dir time={mt} mode={mode:o}")
        elif kind == "link":
            lines.append(f"{path} type=link time={mt} mode={mode:o} link={mtree_quote(name)}")
        else:
            digest = hashlib.sha256(data).hexdigest()
            lines.append(f"{path} time={mt} size={len(data)} mode={mode:o} sha256digest={digest}")
    digest = hashlib.sha256(pkginfo_bytes).hexdigest()
    lines.append(
        f"./.PKGINFO time={mtime} size={len(pkginfo_bytes)} mode=644 sha256digest={digest}"
    )
    return ("\n".join(lines) + "\n").encode()


def add_bytes(tar, name, data, mode, mtime):
    ti = tarfile.TarInfo(name)
    ti.size = len(data)
    ti.mode = mode
    ti.mtime = mtime
    ti.uid = ti.gid = 0
    ti.uname = ti.gname = "root"
    ti.type = tarfile.REGTYPE
    tar.addfile(ti, io.BytesIO(data))


def zstd_compress(raw, out_path):
    """Prefer the Python module; fall back to the zstd CLI.

    The runner installs `zstd` via apt rather than pip-installing a module,
    because Ubuntu 22.04 marks its system Python as externally managed and a
    bare `pip install` there fails.
    """
    try:
        import zstandard
    except ImportError:
        pass
    else:
        with open(out_path, "wb") as f:
            with zstandard.ZstdCompressor(level=19).stream_writer(f) as w:
                w.write(raw)
        return
    res = subprocess.run(["zstd", "-19", "-q", "-f", "-o", out_path, "-"], input=raw)
    if res.returncode != 0:
        die("no zstd available: apt install zstd, or pip install zstandard")


def zstd_decompress(path):
    try:
        import zstandard
    except ImportError:
        pass
    else:
        with open(path, "rb") as f:
            return zstandard.ZstdDecompressor().decompress(f.read(), max_output_size=1 << 30)
    res = subprocess.run(["zstd", "-d", "-q", "-c", path], capture_output=True)
    if res.returncode != 0:
        die(f"cannot decompress {path}: {res.stderr.decode(errors='replace').strip()}")
    return res.stdout


def build(deb_path, out_dir):
    with open(deb_path, "rb") as f:
        members = ar_members(f.read())

    ctrl_blob = next((v for k, v in members.items() if k.startswith("control.tar")), None)
    data_blob = next((v for k, v in members.items() if k.startswith("data.tar")), None)
    if ctrl_blob is None or data_blob is None:
        die("deb is missing control.tar or data.tar")

    work = tempfile.mkdtemp(prefix="deb2arch-")
    try:
        with open_tar_from_bytes(ctrl_blob, "control.tar.gz", work, "control") as ct:
            ctl = {norm(m.name): ct.extractfile(m).read() for m in ct.getmembers() if m.isfile()}

        if "control" not in ctl:
            die("control archive has no `control` file")

        # A deb maintainer script would have to become an Arch .INSTALL. There
        # isn't one today; if Tauri's bundler ever adds a postinst, dropping it
        # silently would ship a package that skips a real install step, so stop.
        for script in ("preinst", "postinst", "prerm", "postrm"):
            if script in ctl:
                die(
                    f"deb contains a {script} maintainer script, which needs an "
                    "Arch .INSTALL equivalent. Hand-convert it rather than "
                    "shipping a package that silently skips the step."
                )

        control = parse_control(ctl["control"].decode("utf-8", "replace"))
        with open_tar_from_bytes(data_blob, "data.tar.gz", work, "data") as dt:
            payload = Payload(dt)

        name = control.get("Package", "unknown")
        version = control.get("Version", "0")
        arch = ARCH_MAP.get(control.get("Architecture", ""), control.get("Architecture", "x86_64"))
        pkgver = f"{version}-{PKGREL}"
        depends = resolve_depends(control, payload)

        # deb Description carries its synopsis then an indented body; .PKGINFO
        # wants only the synopsis. debhelper writes "(none)" as the body when
        # there is no extended description, and that placeholder would otherwise
        # end up in `pacman -Qi` output.
        desc = control.get("Description", "Multiplatform skinnable music player")
        desc = re.sub(r"\s*\(none\)\s*$", "", desc).strip()
        desc = desc.split(" -")[0].strip()

        mtime = int(os.path.getmtime(deb_path))
        pkginfo = [
            "# Generated by deb2arch.py (scripts/deb2arch.py)",
            f"pkgname = {name}",
            f"pkgbase = {name}",
            f"pkgver = {pkgver}",
            f"pkgdesc = {desc}",
            f"url = {URL}",
            f"builddate = {mtime}",
            f"packager = {control.get('Maintainer', 'Unknown Packager')}",
            "buildtype = deb",
            f"size = {control.get('Installed-Size', '0')}",
            f"arch = {arch}",
            f"license = {LICENSE}",
            "",
        ] + [f"depend = {d}" for d in depends]
        pkginfo_bytes = ("\n".join(pkginfo) + "\n").encode()
        mtree_bytes = build_mtree(payload, pkginfo_bytes, mtime)

        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w", format=tarfile.GNU_FORMAT) as tar:
            add_bytes(tar, ".PKGINFO", pkginfo_bytes, 0o644, mtime)
            add_bytes(tar, ".MTREE", mtree_bytes, 0o644, mtime)
            for pname, kind, mode, mt, data in payload.items:
                if kind == "dir":
                    ti = tarfile.TarInfo(pname)
                    ti.type = tarfile.DIRTYPE
                    ti.mode = mode
                    ti.mtime = mt
                    ti.uid = ti.gid = 0
                    ti.uname = ti.gname = "root"
                    tar.addfile(ti)
                elif kind == "link":
                    ti = tarfile.TarInfo(pname)
                    ti.type = tarfile.SYMTYPE
                    ti.linkname = data.decode()
                    ti.mode = mode
                    ti.mtime = mt
                    ti.uid = ti.gid = 0
                    ti.uname = ti.gname = "root"
                    tar.addfile(ti)
                else:
                    add_bytes(tar, pname, data, mode, mt)

        os.makedirs(out_dir, exist_ok=True)
        out = os.path.join(out_dir, f"{name}-{pkgver}-{arch}.pkg.tar.zst")
        zstd_compress(buf.getvalue(), out)

        print(f"deb2arch: wrote {out}")
        print(f"deb2arch: {len(payload.items)} payload entries, depends on {', '.join(depends)}")
        return out
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _unquote(path):
    if not path.startswith('"'):
        return path
    body = path[1:-1]
    return body.replace('\\"', '"').replace("\\\\", "\\")


def verify(pkg_path):
    """
    Re-open a finished package and check it the way pacman will.

    Worth doing in CI: producing a file is not the same as producing a valid
    package, and the failure mode otherwise is a silently broken download that
    nobody notices until an Arch user tries to install it.
    """
    try:
        raw = zstd_decompress(pkg_path)
    except Exception:
        die(f"{pkg_path} is not a readable zstd archive")

    with tarfile.open(fileobj=io.BytesIO(raw)) as tar:
        members = tar.getmembers()
        names = {m.name for m in members}
        for required in (".PKGINFO", ".MTREE"):
            if required not in names:
                die(f"{pkg_path}: {required} missing from the archive")
        mtree = tar.extractfile(".MTREE").read().decode()
        pkginfo = tar.extractfile(".PKGINFO").read().decode()
        blobs = {"./" + m.name: tar.extractfile(m).read()
                 for m in members if m.isfile() and m.name != ".MTREE"}

    problems = []

    # Every digest and size in .MTREE must match the file actually stored.
    listed = set()
    for line in mtree.splitlines():
        m = re.match(r'^("?)(.*?)\1\s+(.*)$', line)
        if not m or not m.group(2).startswith("."):
            continue
        path = _unquote(m.group(2))
        listed.add(path)
        if "sha256digest=" not in m.group(3):
            continue
        blob = blobs.get(path)
        if blob is None:
            problems.append(f"mtree lists {path}, archive does not")
            continue
        want = re.search(r"sha256digest=([0-9a-f]+)", m.group(3)).group(1)
        if hashlib.sha256(blob).hexdigest() != want:
            problems.append(f"digest mismatch for {path}")

    # And nothing may be in the archive without being described.
    for extra in sorted(set(blobs) | {"./" + m.name for m in members if m.isdir()} - listed):
        if extra not in listed:
            problems.append(f"{extra} is in the archive but not in .MTREE")

    if not mtree.startswith("#mtree"):
        problems.append(".MTREE is missing its #mtree header")
    for field in ("pkgname", "pkgver", "arch"):
        if not re.search(rf"(?m)^{field} = \S", pkginfo):
            problems.append(f".PKGINFO has no {field}")
    if any(n.startswith("DEBIAN") for n in blobs):
        problems.append("payload still contains DEBIAN/ control files")
    if [m.name for m in members if m.isfile() and m.mode & 0o002]:
        problems.append("payload contains world-writable files")

    if problems:
        for p in problems:
            print(f"deb2arch: VERIFY FAILED: {p}", file=sys.stderr)
        return 1

    print(f"deb2arch: verified {pkg_path} "
          f"({len(members)} members, {len(listed)} mtree entries)")
    return 0


def main(argv):
    args = [a for a in argv[1:] if a != "--verify"]
    verify_mode = len(args) != len(argv) - 1
    if verify_mode:
        if len(args) != 1:
            print("usage: deb2arch.py --verify <package.pkg.tar.zst>", file=sys.stderr)
            return 2
        return verify(args[0])
    if len(args) != 2:
        print(__doc__)
        print("usage: deb2arch.py <input.deb> <output-dir>", file=sys.stderr)
        return 2
    build(args[0], args[1])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))