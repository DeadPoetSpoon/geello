"""Build the fixed, offline, shared PROJ distribution. Python 3.11+, CMake, C/C++.

Windows uses the Visual Studio 2022 x64 generator; Linux uses Unix Makefiles.
Only explicitly hashed upstream archives are downloaded. No external grids.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=ROOT / "target/m5-native")
    args = parser.parse_args()
    work = args.directory.resolve()
    prefix = work / "install"
    sources = work / "sources"
    sources.mkdir(parents=True, exist_ok=True)
    prefix.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((ROOT / "tools/native-sources.json").read_text())
    assert "zlib" in manifest, "zlib source hash must be recorded before building"
    for spec in manifest.values():
        archive = sources / spec["url"].rsplit("/", 1)[1]
        if not archive.exists():
            with urllib.request.urlopen(spec["url"], timeout=120) as response:
                archive.write_bytes(response.read())
        assert hashlib.sha256(archive.read_bytes()).hexdigest() == spec["sha256"], archive
        if not (sources / spec["directory"]).exists():
            with tarfile.open(archive) as stream:
                stream.extractall(sources, filter="data")
    windows = os.name == "nt"
    cmake = shutil.which("cmake")
    if not cmake and windows:
        cmake = r"C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
    generator = ["-G", "Visual Studio 17 2022", "-A", "x64"] if windows else ["-G", "Unix Makefiles"]
    env = os.environ.copy()
    env["PATH"] = str(prefix / "bin") + os.pathsep + env["PATH"]
    env["CMAKE_PREFIX_PATH"] = str(prefix)
    if not windows:
        env["LD_LIBRARY_PATH"] = str(prefix / "lib")

    def run(*cmd):
        print("+", *cmd, flush=True)
        subprocess.run(cmd, check=True, env=env)

    def build(name, source, *options):
        dest = work / ("build-" + name)
        run(cmake, "-S", str(source), "-B", str(dest), *generator,
            "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_INSTALL_PREFIX=" + str(prefix),
            "-DCMAKE_INSTALL_LIBDIR=lib", "-DCMAKE_PREFIX_PATH=" + str(prefix),
            "-DBUILD_SHARED_LIBS=ON", "-DCMAKE_WINDOWS_EXPORT_ALL_SYMBOLS=ON", *options)
        run(cmake, "--build", str(dest), "--config", "Release", "--parallel", "4")
        run(cmake, "--install", str(dest), "--config", "Release")

    build("zlib", sources / manifest["zlib"]["directory"], "-DZLIB_BUILD_EXAMPLES=OFF")
    sqlite = sources / manifest["sqlite"]["directory"]
    # The amalgamation is public domain. Build the same pinned library and shell,
    # rather than letting CMake find a host SQLite of a different version.
    (sqlite / "CMakeLists.txt").write_text('''cmake_minimum_required(VERSION 3.20)
project(sqlite C)
find_package(Threads REQUIRED)
add_library(sqlite3 SHARED sqlite3.c)
target_compile_definitions(sqlite3 PRIVATE SQLITE_ENABLE_COLUMN_METADATA SQLITE_ENABLE_RTREE)
target_link_libraries(sqlite3 PRIVATE Threads::Threads ${CMAKE_DL_LIBS})
add_executable(sqlite3_cli shell.c)
set_target_properties(sqlite3_cli PROPERTIES OUTPUT_NAME sqlite3)
target_link_libraries(sqlite3_cli PRIVATE sqlite3)
if(UNIX)
target_link_libraries(sqlite3 PRIVATE m)
target_link_libraries(sqlite3_cli PRIVATE m)
endif()
install(TARGETS sqlite3 sqlite3_cli RUNTIME DESTINATION bin LIBRARY DESTINATION lib ARCHIVE DESTINATION lib)
install(FILES sqlite3.h sqlite3ext.h DESTINATION include)
''')
    build("sqlite", sqlite)
    build("tiff", sources / manifest["tiff"]["directory"],
          "-Dtiff-tools=OFF", "-Dtiff-tests=OFF", "-Dtiff-contrib=OFF", "-Dtiff-docs=OFF",
          "-Dtiff-cxx=OFF", "-Djpeg=OFF", "-Djbig=OFF", "-Dlerc=OFF", "-Dlzma=OFF",
          "-Dwebp=OFF", "-Dzstd=OFF", "-Dlibdeflate=OFF", "-Dzlib=ON")
    build("proj", sources / manifest["proj"]["directory"],
          "-DBUILD_TESTING=OFF", "-DENABLE_CURL=OFF", "-DENABLE_TIFF=ON",
          "-DBUILD_PROJSYNC=OFF", "-DBUILD_EXAMPLES=OFF",
          "-DEXE_SQLITE3=" + str(prefix / "bin" / ("sqlite3.exe" if windows else "sqlite3")))
    notices = prefix / "licenses"
    notices.mkdir(exist_ok=True)
    for name, spec in manifest.items():
        source_notice = sources / spec["directory"] / spec["license"]
        if name == "sqlite":
            # Retain the amalgamation's public-domain dedication, not 9 MB of C.
            text = source_notice.read_text(encoding="utf-8")
            (notices / "sqlite.txt").write_text(text[:text.index("*/") + 2] + "\n", encoding="utf-8")
        else:
            shutil.copyfile(source_notice, notices / (name + ".txt"))
    # proj.db and the shipped non-grid support files are pinned individually.
    # Additional grids require an explicitly reviewed source/license/hash entry.
    files = {p.relative_to(prefix).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
             for p in sorted(prefix.rglob("*")) if p.is_file() and p.name != "geello-native.json"}
    imports = sorted((prefix / "lib").glob("proj*.lib")) if windows else [prefix / "lib/libproj.so"]
    assert imports and imports[0].exists(), "missing dynamic PROJ import library"
    output = {"schema_version": 1, "target": "x86_64-pc-windows-msvc" if windows else "x86_64-unknown-linux-gnu",
              "versions": {name: spec["version"] for name, spec in manifest.items()},
              "sources": manifest, "network": False, "grids": [], "files": files,
              "link_name": imports[0].stem if windows else "proj"}
    (prefix / "geello-native.json").write_text(json.dumps(output, indent=2) + "\n")
    print("GEELLO_NATIVE_ROOT=" + str(prefix))


if __name__ == "__main__":
    main()
